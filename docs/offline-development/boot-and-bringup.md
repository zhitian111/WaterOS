# 启动、SMP 与用户态 bring-up 手册

本文用于判断内核停在启动日志的哪一层、初始化函数为何必须处在当前顺序，以及 `make run/shell` 最终会启动什么用户程序。入口源码是 [`src/main.rs`](../../os/src/main.rs)，用户态发布入口是 [`src/user_bringup_bus.rs`](../../os/src/user_bringup_bus.rs)。

## 1. 四个配置维度

| 维度       | 可选值                               | 决定什么                             | 不决定什么                           |
| ---------- | ------------------------------------ | ------------------------------------ | ------------------------------------ |
| `ARCH`     | `rv` / `la`                          | Rust target、平台 feature、QEMU 类型 | 用户态测试队列                       |
| `PROFILE`  | `pre` / `final`                      | 默认镜像与产物名                     | Cargo feature、`auto/shell/run` 模式 |
| `MODE`     | `auto` / `shell` / `run`             | 编译期 `operator-*` feature          | QEMU bootargs                        |
| 根镜像内容 | 是否存在 `/glibc/cagent_testcode.sh` | auto 模式选择初赛或决赛命令队列      | 内核编译 feature                     |

`MODE` 由 [`Makefile`](../../os/Makefile) 转成 `operator-shell` 或 `operator-run` feature。`SCRIPT` 与 `GUEST_SHELL` 通过构建环境进入 `option_env!`，修改后必须重新构建。auto 模式不按 `PROFILE` 猜测试队列，而是在根文件系统挂载后检查镜像标志。复现前先保存：

```sh
make show-config ARCH=rv PROFILE=final MODE=auto
```

## 2. 通用启动阶段

```text
架构汇编入口
  -> wateros_kernel_main
  -> console / logo / klog / runtime logging
  -> init_when_boot
       -> platform::init_when_boot(dtb)
       -> driver::init_when_boot()
  -> runtime heap + arch CPU primitives
  -> init_after_boot
       -> platform::init_after_boot()
       -> probe_and_init_timebase(dtb)
       -> task::init()
       -> task::set_timekeeper_cpu(bsp)
       -> trap_handler::init()
       -> mm::init_after_boot(dtb, memory_end)
       -> task::register_idle_maintenance_hook(mm::idle_maintenance)
  -> 发布并等待 AP online
  -> init_services_after_boot
       -> driver::machine().init_after_boot()
       -> RTC -> wall clock
       -> network::stack::init() -> network_poller_task
       -> fs::init_when_boot() -> fs::init_after_boot()
       -> 可选 self_test
  -> bringup_user_and_optional_services
  -> 开中断、设首个 timer、task::run_first_task()
```

不能随意交换的边界：

- 日志初始化前不能依赖常规日志宏；heap 初始化前不能使用会分配的路径。
- `task::init()` 必须早于需要创建内核任务的网络、GUI 和用户 operator。
- trap handler 必须在开中断和进入用户态前注册。
- MM 需要平台提供物理内存终点；用户 ELF 装载又依赖 MM 和已挂载的根文件系统。
- AP 只有在 BSP 完成共享全局状态后才能加入调度，否则会观察到半初始化状态。
- `user_bringup_bus::run()` 是用户任务的发布边界。

## 3. RISC-V BSP/AP 链路

RISC-V 的入口是 `qemu_riscv64_opensbi::wateros_kernel_main(cpu_raw, dtb_pa, ...)`。

1. 每个 hart 登记当前 CPU 并屏蔽固件遗留中断。
2. `BSP_HART.compare_exchange` 选出唯一 BSP；其余 hart 等待 `AP_BOOT_READY`。
3. BSP 初始化 console、日志、平台、heap、arch、task、trap 与 MM。
4. BSP 以 `Release` 发布 `AP_BOOT_READY`，再通过 SBI HSM 请求启动其他 hart。
5. AP 以 `Acquire` 观察标志，初始化 CPU、IPI、内核页表和中断，登记 online 后进入调度器。
6. BSP 有限自旋等待 `online_cpu_mask` 覆盖请求的 AP；超时 panic，不带残缺 SMP 进入用户态。

| 最后日志                                     | 第一检查点                               |
| -------------------------------------------- | ---------------------------------------- |
| `AP entered Rust` 后停止                     | AP 的 arch/paging/IPI 初始化             |
| `hart_start accepted` 后 `AP online timeout` | `task::set_cpu_online`、OpenSBI HSM      |
| 所有 AP online 后无用户输出                  | 服务初始化返回值、根挂载和 operator 入队 |

## 4. LoongArch BSP/AP 差异

LoongArch 入口不接收可直接使用的 DTB 参数。BSP 先初始化 heap、当前 CPU、arch 与 IPI，再从平台启动信息取得 DTB，并据此建立 configured CPU mask。AP 必须从平台 `_start` 进入，以读取 CPU 编号。

通用逻辑应放进 `init_when_boot`、`init_after_boot` 或 `init_services_after_boot`；只有 CPU 启动运输、页表切换等差异才留在架构模块。至少同时执行：

```sh
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```

## 5. 服务初始化失败语义

`init_services_after_boot()` 只有 machine driver 初始化失败时返回 `false`，此时不会发布用户 workload。RTC 和网络失败只警告：RTC 失败保留无 wall-clock 状态；网络失败不创建 poller，但文件系统与用户态仍可继续。

`network_poller_task` 在持有跨 CPU 网络栈锁期间关闭当前 CPU 全局中断，poll 后恢复原状态并睡眠一个 tick。修改时保证每条返回路径恢复中断，且锁内不发生可调度睡眠。

## 6. 根文件系统与伪文件系统发布

[`user_bringup_bus::run`](../../os/src/user_bringup_bus.rs) 的顺序是：

```text
fs::mount_default_root_rw()
  -> 确保 /proc
  -> 注册 uptime / idle / timer_slack / sysvipc 数据回调
  -> 挂载 procfs
  -> 确保 /sys -> 挂载 sysfs
  -> pre: ensure_busybox_path_links()
  -> user_bringup_busybox::run_stage_busybox()
```

根挂载失败是硬边界：记录错误并返回，不启动用户 ELF。`/proc` 或 `/sys` 单项失败当前是可继续警告。增加伪文件时，应先确认数据所有者已经初始化，并避免在文件系统锁内再取 task/IPC 全局锁；推荐先复制快照再格式化，`sysvipc_table` 即采用此方式。

`user_bringup_mm` 与 `user_bringup_posix_fs` 是可手动启用的烟囱阶段，目前在总线中被注释。临时开启后应在提交前明确保留还是恢复，避免无意改变比赛队列。

## 7. operator 三种模式

[`src/user_operator.rs`](../../os/src/user_operator.rs) 中的 `BootPlan` 是策略真相源。

| 模式    | TTY                             | 执行内容                                 | 退出策略                   |
| ------- | ------------------------------- | ---------------------------------------- | -------------------------- |
| `auto`  | pre 为 fixture，final 为 closed | 按镜像标志选择评测队列                   | 队列结束关机               |
| `shell` | interactive                     | 指定 shell、bash、sh、glibc/musl busybox | shell 退出后继续拉起 shell |
| `run`   | interactive                     | 用候选 shell 执行 `SCRIPT`               | 脚本结束关机               |

interactive 模式会启动 `console_input_main`。Ctrl-C 无效时检查：

```text
vfs::fd::poll_console_input_once
  -> event.process_group / event.signal
  -> syscall::send_kernel_signal_to_process_group
  -> 用户信号投递与 trap 返回
```

shell 候选显式添加 `-i`。否则 SMP 下 shell 可能在控制终端状态发布完成前判断为非交互，表现为阻塞读取但不显示提示符。

## 8. auto 评测队列

[`src/user_bringup_busybox.rs`](../../os/src/user_bringup_busybox.rs) 检查 `/glibc/cagent_testcode.sh`：

- 存在：执行 final 队列 `cagent_testcode.sh`、`buildstorm_testcode.sh`。
- 不存在：执行当前启用的 preliminary 队列，包括 cyclictest、musl LTP、glibc libcbench/lmbench/iozone。

preliminary 队列先处理 LTP 排除项并刷新账号文件。每条命令严格串行：记录开始，装载/创建/等待/回收，记录耗时；非零退出或装载失败终止后续队列。

新增评测命令时同时检查：

1. `program` 是实际 ELF 或带 shebang 的脚本路径。
2. BusyBox 的 `argv[0]` 是 applet 名，不是 BusyBox 路径。
3. glibc/musl 的 `LD_LIBRARY_PATH` 和 LTP `PATH` 匹配镜像。
4. 是否应在前一项失败后继续；当前策略是停止。
5. 自动队列结束必须关机，避免宿主脚本误判超时。

## 9. 单个用户程序生命周期

公共执行函数在 [`src/user_bringup_common.rs`](../../os/src/user_bringup_common.rs)：

```text
run_one_elf_argv_env_exit
  -> 关闭当前 CPU 全局中断
  -> mm::kernel_mm::load_program_from_path
  -> 恢复原中断状态
  -> mm::kernel_mm::prepare_elf_user_stack(argv, envp, auxv)
  -> task::create_user_task（尚未发布）
  -> cred / cwd+fd / env+auxv / mount namespace 挂接
  -> interactive 时设置前台进程组和 controlling session
  -> task::start_user_task
  -> wait_for_task_exit
  -> reap_exited_process（等待整个线程组）
  -> syscall::drop_reaped_task_runtime_resources
  -> purge_all_user_processes（有界兜底）
```

这是“创建但不发布”的事务模式：跨组件资源必须在 `start_user_task` 前挂好。创建失败立即 `drop_user_aspace`；成功退出后，地址空间和 syscall 侧资源只在 reap 后释放。若新增 task-local 子系统，应检查创建挂接、clone/exec 继承、exit 标记、reap 释放四处。

## 10. 常见启动问题

### QEMU host forwarding rule 无法建立

默认把宿主 `127.0.0.1:2222` 转发到 guest 22。该错误发生在 QEMU 启动内核之前，不是 VMA、驱动或网络栈错误。先找占用者：

```sh
lsof -nP -iTCP:2222 -sTCP:LISTEN
```

不需要 SSH 时禁用转发：

```sh
WOS_QEMU_HOSTFWD= make run ARCH=rv PROFILE=final
```

需要 SSH 时换空闲端口：

```sh
WOS_QEMU_HOSTFWD=tcp:127.0.0.1:2223-:22 make run ARCH=rv PROFILE=final
```

### 构建成功但无内核日志

先检查 QEMU 是否真正启动、kernel/image 路径和架构，再检查极早 console 与链接入口。此阶段还未进入 VFS 或用户态，不应从 syscall 排查。

### 有根挂载日志但无 shell/测试

检查 `stage-busybox`、`operator plan` 日志，核对 `MODE` 是否重新构建生效；auto 模式再检查镜像标志和命令 ELF。

### 用户命令结束后不进入下一项

检查线程组是否全部 `Exited`、`reap_exited_process` 能否原子回收，以及阻塞 syscall 是否被 `exit_group` 唤醒。不要只观察 leader 退出码。

## 11. 最小回归

```sh
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
make show-config ARCH=rv PROFILE=final MODE=shell
make shell ARCH=rv PROFILE=final SMP=1
make shell ARCH=rv PROFILE=final SMP=8
```

需要自动队列时再用匹配镜像运行 `MODE=auto`。SMP=1 验证基本顺序，SMP=8 验证发布屏障、per-CPU 初始化和就绪队列；两者不能互相替代。

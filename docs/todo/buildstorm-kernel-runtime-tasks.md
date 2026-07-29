# BuildStorm 内核运行时兼容任务

## 范围与结论

本文记录 2026-07-27 在 RISC-V64、QEMU `virt`、8 核、8 GiB 和
`os/sdcard-rv-pub.img` 上运行 `/glibc/buildstorm_testcode.sh` 后确认的内核任务。
测试使用 qcow2 overlay，原镜像未被修改。

## 2026-07-29 最新状态

以下结论优先于本文后续的历史任务描述：

- CAgent 已连续三轮通过全部 10 项测试。
- `rustc --version`、`cargo --version` 和 `cargo new && cargo build` 均已通过，
  `BUILDSTORM_TOOLCHAIN ok`、`BUILDSTORM_MINIBUILD ok` 已稳定出现。
- Rust 使用的 `SOCK_SEQPACKET | SOCK_CLOEXEC` jobserver 通道已接入 Unix socket
  side table；`sendto/recvfrom` 支持已连接端点，阻塞 I/O 不再持有 socket 元数据锁。
- `execve` 会同步清理 VFS fd table 与 Unix socket side table 中的 `CLOEXEC` 引用，
  父进程可正确观察 jobserver 管道 EOF。
- 当前参考镜像不能解析完整负载：`/work/tgoskits/Cargo.lock` 锁定
  `web-sys 0.3.103`，镜像离线索引最高仅含 `0.3.94`。这是镜像内容不一致，不是
  WaterOS 的文件读取错误。
- 8 crate、8 job 并行探针已完成一轮：`rc=0 built=8 elapsed_s=566.82`；随后
  CAgent 10/10 通过，运行后镜像通过 `e2fsck -fn` 五阶段检查。
- 已修复并行编译暴露的 ext4 目录扩容、同目录 rename link count、页缓存脏页淘汰
  错误串扰和页安装/淘汰 TOCTOU 竞态。
- 已修复 `exit_group` 提前把远端线程标记为 Exited 的竞态；进程只有在所有线程实际
  退出后才进入可 reap 状态。

在取得依赖完整的评测镜像前，使用
`os/scripts/guest_buildstorm_parallel_probe.sh` 验证 8 crate、8 job 的
`clone/exec/wait/futex/socketpair/pipe/file I/O` 并发链路。禁止通过修改锁文件或
伪造 Cargo 成功结果绕过镜像问题。

## 当前推进条目

### E1. 并行编译正确性

- [ ] 将 guest probe 注入临时镜像副本，连续运行至少三轮（当前完成 1/3）。
- [ ] 每轮必须出现 `BUILDSTORM_PROBE_END rc=0 built=8`，且无 panic、死锁和用户任务
      永久阻塞。
- [x] 按首个错误 syscall、fault 或等待对象定位；已按根因独立修复、验证和提交。

### E2. IPC 与调度热点

- [ ] 统计 probe 中 futex wait/wake、远端 reschedule IPI、上下文切换和 syscall 数。
- [ ] 审核 futex registry 与 scheduler 全局锁持有区；只优化有测量证据的热点。
- [ ] 确保 `FUTEX_WAIT/WAKE/REQUEUE` 的无丢失唤醒协议以及
      `CLONE_CHILD_CLEARTID`、robust futex 退出清理不回归。

### E3. 文件与进程调用链

- [x] 记录 8 crate probe 的墙钟时间，定位 ext4、页缓存、fork/exec 或 fd table 的
      主要耗时。
- [ ] 优先消除全局锁下的阻塞 I/O、重复路径解析和逐页/逐小块串行操作。
- [ ] 每项优化前后使用相同 overlay、8 核和 8 GiB 配置对比。

### E4. 最终验收

- [x] `make kernel-rv-final` 通过。
- [x] CAgent 10/10 至少再回归一轮。
- [ ] 初赛脚本不出现新增 panic、卡死或关键 syscall 语义回归。
- [ ] 取得依赖完整镜像后运行官方 BuildStorm，要求正式产物不少于 500 KiB。

镜像中的 `/root/.cargo/bin/rustc`、`cargo` 均为指向同目录 `rustup` 的相对符号
链接，内核已经能够解析链接并启动 `rustup`。当前失败不是 ext4 镜像损坏，而是
进程启动后的 syscall、procfs 和 fd 语义不完整。

## A. P0：工具链启动阻断

### A1. 实现 `readlinkat` 与 `/proc/<pid>/exe`

**问题：** `sys_readlinkat` 当前固定返回 `ENOSYS`。`rustup` 启动后需要读取
`/proc/self/exe`，最终以 code 38（Function not implemented）退出。

**更改：**

- 按 `dirfd` 解析路径，但不跟随最终符号链接。
- 实现 Linux `readlinkat` 的原始字节复制、截断、`EFAULT`、`EINVAL`、`ENOENT`、
  `EBADF` 和 `ENOTDIR` 语义；返回复制的字节数且不追加 NUL。
- 在 procfs 中增加 `/proc/self/exe`、`/proc/<pid>/exe` 符号链接节点。
- 目标由已有 task executable-path registry 提供；进程退出后节点不可见。
- 让 fs-bridge 的 proc 路由支持 `read_symlink`，不要写 syscall 特判绕过 VFS。

**涉及文件：**

- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/dir.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/path_at.rs`
- `os/components/wateros-fs/fs-procfs/procfs-api/api-v0/src/lib.rs`
- `os/components/wateros-fs/fs-procfs/procfs-impl/impl-kernel/src/lib.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/lib.rs`
- `os/components/wateros-vfs/src/cwd.rs`

**验收：**

- `readlink /proc/self/exe` 返回当前实际可执行文件路径。
- 相对 symlink、短 buffer、无效 fd 和非 symlink 回归通过。
- `rustc --version`、`cargo --version` 不再因 `ENOSYS` 退出。

### A2. 实现 `eventfd2(19)`

**问题：** `rustup` panic 前调用了 `eventfd2`，当前未进入 syscall 分发。Rust
运行时、并行构建工具及 epoll 事件通知可能依赖 eventfd。

**更改：**

- 新增共享 64 位计数器 fd，支持 8 字节 read/write。
- 实现阻塞、`EFD_NONBLOCK`、`EFD_CLOEXEC`、`EFD_SEMAPHORE`、溢出与错误语义。
- 接入现有 fd table、wait queue、`poll`/`ppoll`/`epoll` readiness。
- fork 共享 open-file description；exec 按 CLOEXEC 关闭。

**涉及文件：**

- `os/components/wateros-syscall/syscall-api/api-v0/src/lib.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/syscall_nr_dispatch.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/`
- `os/components/wateros-vfs/vfs-api/api-v0/src/handle.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fd-session/src/`

**验收：** 单线程计数、非阻塞、semaphore、fork 共享及 epoll 唤醒测试通过，且
BuildStorm 工具链探针不再在 `eventfd2` 后 panic。

## B. P1：Rust/glibc 线程运行时

### B1. 实现 `sigaltstack(132)`

**问题：** rustup 已调用该接口；固定 `ENOSYS` 会削弱栈溢出和崩溃信号处理。

**更改：** 为每线程保存 alternate signal stack，校验 `SS_DISABLE`、栈范围和
最小尺寸；信号派发在 `SA_ONSTACK` 时切换用户栈，并在 signal return 后恢复。

**涉及文件：**

- `os/components/wateros-syscall/syscall-api/api-v0/src/lib.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/signal.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/`
- task 的 per-thread signal state 定义与 clone/exit 清理位置

### B2. 兼容 `rseq(293)`

**问题：** glibc 为多个进程/线程重复尝试注册 rseq。`ENOSYS` 通常可回退，但会
禁用用户态快速路径并影响并行构建性能。

**更改：** 第一阶段可明确返回 Linux 可识别的兼容结果并限频记录；第二阶段实现
per-thread rseq 注册、长度/signature 校验、CPU ID 更新和调度抢占时的 abort。

**涉及文件：**

- `os/components/wateros-syscall/syscall-api/api-v0/src/lib.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/syscall_nr_dispatch.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/`
- task scheduler 的迁移、抢占和上下文切换路径

### B3. 兼容 RISC-V `riscv_hwprobe(258)`

**问题：** glibc 在进程启动阶段探测 ISA/CPU 能力。当前每次返回 `ENOSYS`，虽可
回退，但会丢失优化能力并产生大量探测。

**更改：** 增加架构 syscall 常量和 RISC-V 实现，至少报告 vendor/arch/impl ID、
基础扩展与 cache block size；校验 pair 数组、CPU mask 和 flags。LoongArch 构建
不得暴露此 syscall。

**涉及文件：**

- `os/components/wateros-syscall/syscall-api/api-v0/src/lib.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/syscall_nr_dispatch.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/arch/`（建议新增）
- `os/components/wateros-platform/` 的 CPU/ISA 查询接口

## C. P1：构建过程的文件与目录语义

### C1. 实现 `fchdir(50)`

**问题：** cargo 在构建过程中使用目录 fd；当前缺失会破坏基于已打开目录切换 cwd
的流程。

**更改：** 校验 fd 存在且指向目录，将当前 task cwd 原子更新为该 handle 对应的
绝对路径，并保持 fork/clone 的 cwd 复制规则。

**涉及文件：**

- `os/components/wateros-syscall/syscall-api/api-v0/src/lib.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/cwd.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fd-session/src/handles.rs`
- `os/components/wateros-vfs/src/cwd.rs`

### C2. 实现 `fadvise64(223)` 兼容语义

**问题：** 编译器和链接器会对大文件给出访问模式提示。该调用不应让构建失败。

**更改：** 校验 fd、offset、length 和 advice；对普通文件的合法 advice 先返回
成功 no-op，非法 advice 返回 `EINVAL`，非 seekable fd 按 Linux errno 返回。后续
可把 `DONTNEED/WILLNEED` 接入页缓存。

**涉及文件：**

- `os/components/wateros-syscall/syscall-api/api-v0/src/lib.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/`
- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/` 的页缓存接口

### C3. 实现 POSIX timer（从 `timer_create(107)` 开始）

**问题：** 诊断日志确认用户程序调用 `timer_create`，当前未分发。BuildStorm 使用
`timeout 14400 ...` 包裹编译命令，缺失 POSIX timer 可能使超时管理和子进程终止
行为失效。

**更改：** 实现每进程 timer ID 表以及 `timer_create`、`timer_settime`、
`timer_gettime`、`timer_getoverrun`、`timer_delete`；先支持
`CLOCK_REALTIME/CLOCK_MONOTONIC` 和 `SIGEV_SIGNAL`，复用现有单调时钟、定时唤醒与
signal 投递。并发删除、进程退出清理和周期 timer 重装必须持有正确的同步保护。

**涉及文件：**

- `os/components/wateros-syscall/syscall-api/api-v0/src/lib.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/syscall_nr_dispatch.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/time/`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/signal.rs`
- task 的进程资源创建、fork/exec/exit 清理路径

**验收：** 单次、周期、查询、删除和信号通知测试通过；guest 内
`timeout 1 sleep 10` 能按预期终止且返回 124。

## D. P2：环境兼容与日志噪声

### D1. 处理 `fsopen(430)`

测试脚本尝试挂载 procfs、sysfs 和 devtmpfs，并忽略错误。WaterOS 已在 bringup
挂载 procfs，因此 `fsopen` 不是当前工具链门槛。短期保持 `ENOSYS`，但应限频日志；
若需要新 mount API，再实现 `fsopen/fsconfig/fsmount/move_mount` 整套 fd 状态机，
不能只伪造 `fsopen` 成功。

### D2. 修正 TTY `TCGETS(0x5401)`

日志显示 fd 1/2 的 `TCGETS` 返回 `ENOTTY`。这不会直接导致本次 rustup panic，但会
影响终端检测、彩色输出和交互程序。

检查 fd 继承后是否仍保留 TTY character-handle 标记；为真实 UART fd 返回稳定的
Linux `termios`，pipe/file 仍返回 `ENOTTY`。涉及：

- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/misc/ioctl.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fd-session/src/char_dev_handle.rs`
- fd fork/dup/exec 复制路径

### D3. 决赛构建关闭 dashboard 和高频诊断日志

**问题：** `dashboard-debug` 虽标注为默认关闭，却被
`qemu-riscv64-opensbi` feature 直接启用。每 500 ms 输出整张表会占用全局 UART
锁、拖慢 QEMU 中的并行编译，并把无关内容写入评测串口。

**更改：** 从架构 feature 中移除 `dashboard-debug`，仅在专用调试构建显式启用；
为 `rv_final_run` 保持关闭。将可回退 syscall（例如 hwprobe/rseq/fsopen）日志改为
trace 或限频统计，错误路径仍保留 warn。

**涉及文件：**

- `os/Cargo.toml`
- `os/Makefile`
- `os/src/dashboard.rs`
- `os/src/main.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/syscall_nr_dispatch.rs`

**验收：** `make kernel-rv-final` 的 feature tree 不含 `dashboard-debug`，运行日志
不再周期性输出 dashboard；工具链错误和评测标记仍完整可见。

## 执行顺序与统一验收

1. A1 `readlinkat + /proc/<pid>/exe`
2. A2 `eventfd2`
3. B1 `sigaltstack`
4. C1 `fchdir`、C2 `fadvise64`、C3 POSIX timer
5. B2 `rseq`、B3 `riscv_hwprobe`
6. D3 决赛日志收敛，再处理 D1/D2

每条任务单独提交；至少执行 `make kernel-rv-final`。运行验证必须使用
`os/sdcard-rv-pub.img` 的临时 overlay，依次达到：

```text
BUILDSTORM_TOOLCHAIN ok
BUILDSTORM_MINIBUILD ok
BUILDSTORM_COMPILE mode=multi ok=true ...
```

在出现下一处失败时记录 syscall 号、参数、返回值和触发程序，再新增任务；不得把
“syscall 已有分发项”直接视为语义已经完整。

# WaterOS 内核快速使用

WaterOS 同时支持 RISC-V 和 LoongArch QEMU。Make 是面向操作者的
统一入口；Python 脚本作为调试工具的底层和高级接口。

以下命令均在 `os/` 目录执行：

```bash
cd /home/kasss/WaterOS/os
make help
make show-config
```

`make help` 显示入口命令和主要默认值；`make show-config` 显示经过
`ARCH/PROFILE`、环境变量和命令行覆盖后最终生效的内核、镜像与运行参数。例如：

```bash
make show-config ARCH=rv PROFILE=final SMP=4
make show-config ARCH=la PROFILE=pre LA_PRE_IMAGE=/data/la-pre.img
```

## 1. 构建与自动评测

统一变量的默认值是 `ARCH=rv PROFILE=pre SMP=8 MODE=auto
SNAPSHOT=1`。

```bash
# 构建或检查
make build ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=final

# 运行内核中现有的 BRINGUP_COMMANDS
make run ARCH=rv PROFILE=pre
make run ARCH=rv PROFILE=final
make run ARCH=la PROFILE=pre
make run ARCH=la PROFILE=final
```

Make 不扫描镜像、不维护测试目录，也不提供 `make tests` 或
`make test TEST=...`。`MODE=auto` 执行什么由内核现有 bringup
配置决定。额外 Cargo feature 可通过通用变量传入：

```bash
make run ARCH=rv PROFILE=pre EXTRA_FEATURES=bringup-ltp-glibc-only
```

## 2. 进入交互终端

```bash
make shell ARCH=rv PROFILE=pre SMP=1
make shell ARCH=la PROFILE=final SMP=4
```

启动后看到 `~ #` 或 `/ #` 表示已进入镜像内的用户态 shell。
这个提示符由 Bash/BusyBox ash 绘制，TTY 负责 UART 字节、行编辑、
echo、raw mode 和 Ctrl-C 等终端语义。

常用 guest 命令：

```sh
pwd
ls -la /
cd /glibc
/glibc/busybox sh /glibc/iperf_testcode.sh
echo hello > /tmp/hello.txt
cat /tmp/hello.txt
sleep 30                 # 可按 Ctrl-C 中断
```

指定 shell 时使用 `GUEST_SHELL`，不要使用 Make 保留变量 `SHELL`：

```bash
make shell ARCH=rv PROFILE=final GUEST_SHELL=/glibc/busybox
```

更完整的 TTY、Ctrl-C、raw mode、救援 shell 和排查说明见
[`wateros-tty/README.md`](./components/wateros-tty/README.md)。

## 3. 运行镜像内指定脚本

`MODE=run` 保留为通用 operator 能力，它不是 Make 测试目录。

```bash
make run ARCH=rv PROFILE=final \
  MODE=run \
  SCRIPT=/glibc/iperf_testcode.sh
```

`SCRIPT` 必须是 guest 中的绝对路径，且只能和 `MODE=run` 一起使用。
脚本成功、失败或装载失败后，supervisor 都会进入救援 shell。

## 4. 可选图形欢迎页

图形显示默认关闭，不影响比赛构建。下面的命令会同时编译 `display-demo`
feature、给 QEMU 挂载 VirtIO GPU，并打开 GTK 窗口：

```bash
make run ARCH=rv PROFILE=pre EXTRA_FEATURES=display-demo
make run ARCH=la PROFILE=pre EXTRA_FEATURES=display-demo
```

图形窗口显示 WaterOS 欢迎页，原终端仍承载 UART 日志和交互 shell。当前版本只验证
GPU、DMA framebuffer、软件绘制和 `flush()` 通路，还没有图形终端、鼠标或窗口管理器。
显示驱动结构见
[`driver-display/README.md`](./components/wateros-driver/driver-display/README.md)。

无桌面环境时，可以保留 GPU 设备但隐藏窗口，用于启动回归：

```bash
make run ARCH=rv PROFILE=pre \
  EXTRA_FEATURES=display-demo \
  GRAPHICS_BACKEND=none
```

如果只设置 `GRAPHICS=1` 而不编译 `display-demo`，QEMU 虽然会挂 GPU，内核不会绑定
它；反之，`display-demo` 会默认令 `GRAPHICS=1`，仍可显式用 `GRAPHICS=0` 覆盖。

## 5. 磁盘 snapshot

统一入口默认 `SNAPSHOT=1`。QEMU 会把 guest 的磁盘写入保存在内存/
临时层，退出时丢弃；它不是先复制整个 `.img`，也不会改动基础
镜像。

只有明确需要保存 guest 文件时才开启写盘：

```bash
make shell ARCH=rv PROFILE=pre WRITE_DISK=1
```

`WRITE_DISK=1` 会默认把 `SNAPSHOT` 切换为 `0`。仍可显式指定
`SNAPSHOT=1` 强制不写盘。自动评测、回归和 GDB 调试不应写回基础
镜像。

## 6. GDB 自动挡

```bash
make doctor
make debug ARCH=rv PROFILE=pre SMP=8
```

`make debug` 会：

1. 构建独立的 `kernel-*-gdb` ELF；
2. 以 snapshot 模式启动 QEMU 并立即运行；
3. 每秒监测 CPU、PC、timer、调度、事件和锁；
4. 确认停滞后暂停 guest，生成 `debug-reports/` 报告；
5. 保留活动会话，可再执行 `make gdb`。

自动挡终端显示 watch 摘要，完整串口保存在
`debug-reports/active/*.log`。

## 7. GDB 手动挡（两个终端）

终端一：

```bash
cd /home/kasss/WaterOS/os
make debug-server ARCH=rv PROFILE=pre SMP=8
```

默认 `START_PAUSED=1`，QEMU 停在复位入口，因此连接 GDB 前可能没有
串口输出。

终端二：

```bash
cd /home/kasss/WaterOS/os
make gdb
```

GDB 连接后可执行：

```gdb
break wateros_kernel_main
continue
wos-cpus
wos-tasks
wos-events
wos-locks
thread apply all bt full
```

需要让内核先运行，再在可疑时刻连接：

```bash
# 终端一
make debug-server ARCH=rv PROFILE=pre START_PAUSED=0

# 终端二，需要时执行
make gdb
```

## 8. 附加已运行的调试会话

`debug-server` 会把 QEMU PID、架构、profile、ELF、build ID、端口和
串口日志记录到 `debug-reports/active/session.json`。因此第二个终端
无需重复参数：

```bash
make snapshot                 # 抓取现场后默认继续运行
make snapshot LEAVE_STOPPED=1
make watch                    # 附加自动停滞检测
make gdb                      # 交互式调试
```

会话过期、PID 不存在、ELF 被重建或 build ID 不匹配时，工具会拒绝
继续并要求重新执行 `make debug` 或 `make debug-server`。

完整 GDB 命令、报告结构、确定性故障注入和底层原理见
[`GDB_STALL_DEBUG.md`](../docs/debugging/GDB_STALL_DEBUG.md)。

## 9. 常用变量

下表记录 Makefile 中的静态默认值。需要确认某次命令实际会使用什么，请执行
`make show-config` 并传入与运行命令相同的变量。

| 变量 | 默认 | 作用 |
| --- | --- | --- |
| `ARCH` | `rv` | `rv` / `la` |
| `PROFILE` | `pre` | `pre` / `final` |
| `SMP` | `8` | QEMU vCPU 数，`1..8` |
| `MODE` | `auto` | `auto` / `shell` / `run` |
| `SCRIPT` | 空 | `run` 模式的 guest 绝对路径 |
| `GUEST_SHELL` | 空 | 指定 guest shell/BusyBox ELF |
| `TTY` | 由内核决定 | `interactive` / `closed` / `fixture` |
| `LOG` | 由内核决定 | `error` / `warn` / `info` / `debug` / `trace` |
| `ON_EXIT` | 由模式决定 | `shutdown` / `shell` / `reboot` |
| `SNAPSHOT` | `1` | `1` 不写回基础镜像 |
| `WRITE_DISK` | `0` | `1` 允许写回，并默认关闭 snapshot |
| `PORT` | `1234` | QEMU GDB Remote 端口 |
| `START_PAUSED` | `1` | `debug-server` 是否传入 `-S` |
| `FAULTS` | `0` | 编译确定性故障注入钩子 |
| `RV_PRE_IMAGE` | `./sdcard-rv.img` | RISC-V pre 默认镜像 |
| `RV_FINAL_IMAGE` | `./sdcard-rv.img` | RISC-V final 默认镜像，可独立覆盖 |
| `LA_PRE_IMAGE` | `./sdcard-la.img` | LoongArch pre 默认镜像 |
| `LA_FINAL_IMAGE` | `./sdcard-la.img` | LoongArch final 默认镜像 |
| `SDCARD` | 由上述四项选择 | 覆盖本次运行的镜像路径 |
| `EXTRA_FEATURES` | 空 | 逗号分隔的额外根 crate feature |
| `GRAPHICS` | `EXTRA_FEATURES` 含 `display-demo` 时为 `1`，否则 `0` | 是否挂载 QEMU VirtIO GPU 并启用图形输出 |
| `GRAPHICS_BACKEND` | `auto` | QEMU display backend，`auto` 会按宿主/QEMU 支持自动选择，也可显式指定 `gtk`、`sdl`、`cocoa` 或 `none` |

镜像名称只在 Makefile 的四个 `*_IMAGE` 变量中定义，QEMU 启动脚本不会
根据 profile 猜测镜像，也不会让 final 静默回退到 pre 镜像。可以永久修改
Makefile 中的默认值，也可以只覆盖一次：

```bash
# 临时替换一次 final 镜像
make run ARCH=rv PROFILE=final SDCARD=/data/wateros-final.img

# 为本次 Make 调用修改该组合的默认值
make run ARCH=rv PROFILE=pre RV_PRE_IMAGE=/data/wateros-pre.img
```

## 10. 常见问题

- `debug-server` 没有串口输出：默认停在复位入口，在第二个终端
  `make gdb` 后执行 `continue`。
- 端口被占用：在终端一传入 `PORT=1235`；终端二会从会话自动
  读取，不用再传。
- shell 无输入：使用 `make shell`，不要设置 `TTY=closed`。
- 提示符被日志覆盖：使用 `LOG=warn` 或 `LOG=error`。
- 退出 QEMU：按 `Ctrl-A` 后按 `x`。Guest 内的 Ctrl-C 仍用于中断前台进程。
- 图形窗口未出现：确认使用了 `EXTRA_FEATURES=display-demo`，并检查宿主是否安装了
  对应的 QEMU 显示后端；服务器环境可先用 `GRAPHICS_BACKEND=none` 验证驱动。

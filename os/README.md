# WaterOS 现场操作、内核构建与 GDB 诊断

本目录是 WaterOS 内核工作目录。下面重点说明 RISC-V、LoongArch 两套 GDB 调试
流程，用于分析内核卡死、死循环、锁等待、调度停滞、中断停止、IPI/TLB shootdown
等待等问题。

所有命令默认在本目录执行：

```bash
cd /home/kasss/WaterOS/os
```

## 现场 operator 模式

内核现在从启动参数选择自动评测、交互 shell 或指定脚本，无需修改
Rust 源码或 Cargo feature。不传参数的 `rv_final_run` / `la_final_run` 仍执行原
自动测例队列并在结束后关机。

最常用的四条现场命令：

```bash
# RISC-V / LoongArch 交互 shell（默认 snapshot，不写回镜像）
make rv_final_run WOS_MODE=shell
make la_final_run WOS_MODE=shell

# 运行镜像内脚本，完成或失败后都进入救援 shell
make rv_final_run WOS_MODE=run WOS_SCRIPT=/root/test.sh
make la_final_run WOS_MODE=run WOS_SCRIPT=/root/test.sh
```

shell 候选顺序是 `WOS_SHELL` 指定路径、`/bin/bash`、`/bin/sh`、
`/glibc/busybox sh`、`/musl/busybox sh`。shell 意外退出后 supervisor 会清理残留
子进程并再次启动救援 shell。operator 默认日志级别为 `warn`，避免 SMP
日志覆盖提示符。

### 从构建到进入终端

运行目标会自动构建对应内核，不需要先单独执行 `make kernel-*`。第一次验证建议先用
单核，确认串口和镜像正常后再把 `WOS_SMP` 改为 `2/4/8`：

```bash
cd /home/kasss/WaterOS/os

# 初赛镜像
make rv_pre_run WOS_MODE=shell WOS_SMP=1 WOS_LOG=warn
make la_pre_run WOS_MODE=shell WOS_SMP=1 WOS_LOG=warn

# 决赛镜像
make rv_final_run WOS_MODE=shell WOS_SMP=1 WOS_LOG=warn
make la_final_run WOS_MODE=shell WOS_SMP=1 WOS_LOG=warn
```

启动完成后会看到类似提示符：

```text
~ #
```

这已经是镜像内的用户态 shell，不是 QEMU monitor，也不是 WaterOS 内核命令行。
当前比赛镜像可能没有独立的 Bash 或 `/bin/sh`，supervisor 会自动回退到
`/glibc/busybox sh` 或 `/musl/busybox sh`。可以用下面的命令确认实际环境：

```sh
echo "$SHELL"
echo "$PATH"
pwd
id
uname -a
```

若要固定使用某个 shell：

```bash
make rv_final_run WOS_MODE=shell WOS_SHELL=/glibc/busybox
make la_final_run WOS_MODE=shell WOS_SHELL=/musl/busybox
```

指定路径装载失败时，内核会继续尝试后续候选并打印具体错误；所有候选都失败时不会
自动关机，仍可连接 GDB 查看现场。

### 进入终端后的常用操作

普通文件、管道、重定向和目录操作：

```sh
cd /root
ls -la /
echo "hello WaterOS"
echo "hello WaterOS" > /tmp/hello.txt
cat /tmp/hello.txt
cat /etc/passwd | wc -l
mkdir -p /tmp/demo
cp /tmp/hello.txt /tmp/demo/
```

创建并执行脚本：

```sh
printf '#!/bin/sh\necho script-start\npwd\necho script-end\n' > /root/test.sh
chmod +x /root/test.sh
"$SHELL" /root/test.sh
```

后台任务和 job control：

```sh
sleep 10 &
jobs
wait

# 前台命令可按 Ctrl-C 中断，随后应重新出现提示符
sleep 30
```

`Ctrl-C` 会向前台进程组发送 `SIGINT`，`Ctrl-Z` 会发送 `SIGTSTP`。是否显示完整的
`jobs/fg/bg` 行为还取决于镜像中 shell 的 job-control 支持。

检查或恢复终端状态时优先直接调用 BusyBox applet，因为部分镜像没有安装独立的
`stty` 链接：

```sh
busybox stty -a
busybox stty raw -echo
busybox stty sane
```

如果程序异常退出后提示符不回显或输入显示混乱，可先盲输 `busybox stty sane` 并按
回车。若仍无效，先按 `Ctrl-A`，再按小写 `x` 退出 QEMU 后重新启动。这个组合是 QEMU
`-nographic` 控制键；`Ctrl-C` 才是发送给 guest 前台程序的终端信号。

当前 pre 镜像实测没有 Vim。使用前先检查：

```sh
command -v vim
```

只有镜像确实包含 Vim 时，才能执行 `vim /root/test.txt`；内核 TTY 已支持 Vim 需要的
raw 模式和 `VMIN/VTIME`，但用户程序本身仍必须存在于磁盘镜像中。

### 自动评测、脚本和退出策略

保持原有自动评测和自动关机行为：

```bash
make rv_final_run
make la_final_run

# 等价的显式写法
make rv_final_run WOS_MODE=auto
```

`run` 模式要求脚本在启动前已经存在于镜像中。脚本成功、失败或无法装载后都会进入
救援 shell：

```bash
make rv_final_run WOS_MODE=run WOS_SCRIPT=/root/test.sh
make la_final_run WOS_MODE=run WOS_SCRIPT=/root/test.sh
```

默认 `shell` 模式下执行 `exit` 不会关机，而是由 supervisor 清理残留用户进程并重新
启动一个救援 shell。需要通过 `exit` 关机或重启时，在启动 QEMU 时指定：

```bash
make rv_final_run WOS_MODE=shell WOS_ON_EXIT=shutdown
make rv_final_run WOS_MODE=shell WOS_ON_EXIT=reboot
```

QEMU 运行脚本带有 `-no-reboot`，因此 `reboot` 策略会让当前 QEMU 进程退出，而不是在
同一个终端里再次启动 guest；重新运行 `make ..._run` 即可开机。

可用启动参数：

| Make 变量 | 内核参数 | 取值 / 作用 |
| --- | --- | --- |
| `WOS_MODE` | `wos.mode` | `auto` / `shell` / `run` |
| `WOS_SHELL` | `wos.shell` | 绝对 shell/BusyBox 路径 |
| `WOS_SCRIPT` | `wos.script` | `run` 模式的绝对脚本路径 |
| `WOS_ON_EXIT` | `wos.on_exit` | `shutdown` / `shell` / `reboot` |
| `WOS_TTY` | `wos.tty` | `interactive` / `closed` / `fixture` |
| `WOS_LOG` | `wos.log` | `error` / `warn` / `info` / `debug` / `trace` |
| `WOS_SMP` | platform hint | QEMU vCPU 数量（`1..8`；LoongArch 同时用于限制 mailbox 目标） |

`interactive` 从真实 UART 读取；`closed` 立即 EOF；`fixture` 只为原无人值守
LTP 密码输入提供 `password\n`。已知 `wos.*` 参数的非法值会打印错误并
进入救援 shell，不会静默跑评测。完全没有 bootargs 时串口显示 3 秒菜单，
超时保持当前构建的默认自动模式。

operator 模式默认向 QEMU 加 `-snapshot`。只有明确要保存文件时才写回基础
镜像：

```bash
make rv_final_run WOS_MODE=shell WOS_WRITE_DISK=1
make la_final_run WOS_MODE=shell WOS_WRITE_DISK=1
```

默认 snapshot 下，`/root` 等磁盘文件的修改会在 QEMU 退出后消失；`/tmp` 是 tmpfs，
即使启用写盘也不会跨重启保存。如果想创建一个供下一次 `WOS_MODE=run` 使用的脚本：

1. 使用 `WOS_MODE=shell WOS_WRITE_DISK=1` 启动。
2. 在 `/root` 创建脚本并执行 `sync`。
3. 退出 QEMU，然后以 `WOS_MODE=run WOS_SCRIPT=/root/...` 再次启动。

基础镜像用于评测或回归时不要打开 `WOS_WRITE_DISK=1`，避免一次现场操作污染后续结果。

自动评测为保持原行为不强制 snapshot；需要只读回归时显式传
`WOS_QEMU_SNAPSHOT=1`。

### TTY 能力和排查

控制台实现 canonical/raw 输入、echo、`VMIN/VTIME`、`poll/select` 可读性、
`TCGETS/TCSETS*`、窗口大小、控制终端与前台进程组。Ctrl-C/Ctrl-Z 分别向
前台进程组投递 `SIGINT`/`SIGTSTP`；后台读控制终端会收到 `SIGTTIN`，
`TOSTOP` 下后台写会收到 `SIGTTOU`。

- 没有输入：确认使用 `WOS_MODE=shell`，不要设 `WOS_TTY=closed`；检查 QEMU
  命令包含 `-nographic` 且没有其他进程占用串口。
- 提示符被日志打乱：用 `WOS_LOG=warn` 或 `WOS_LOG=error`；不要在交互时开 CPU
  dashboard。
- Ctrl-C 无效：用 `busybox stty -a` 检查 `isig` 和 `intr = ^C`，再检查
  `tcgetpgrp`/shell job control；可立即用下面的 GDB snapshot 查任务和锁。
- Bash 装载失败：日志会列出每个候选及 ELF loader 错误；尝试
  `WOS_SHELL=/glibc/busybox`。所有候选都失败时内核保持运行，便于附加 GDB。

一边现场操作一边开放 GDB：

```bash
# 终端 1：立即运行 shell，保持磁盘 snapshot
make rv_final_run-gdb GDB_WAIT=0 WOS_MODE=shell

# 终端 2：立即保存现场（默认抓取后继续运行）
./scripts/wateros_debug.py snapshot --arch rv --elf ./kernel-rv-final-gdb

# 或持续监测，确认卡死后生成报告并保持暂停
./scripts/wateros_debug.py watch --arch rv --elf ./kernel-rv-final-gdb
```

LoongArch 将 `rv`/`kernel-rv` 替换为 `la`/`kernel-la`即可。上述命令、镜像、
ELF 和 Python 脚本均在仓库内，可断网使用。

断网自动交互 smoke（默认 snapshot）：

```bash
./scripts/operator_smoke.py --arch rv --profile pre --smp 1
./scripts/operator_smoke.py --arch la --profile pre --smp 1
./scripts/operator_smoke.py --arch rv --profile final --smp 4 --vim

# 脚本不存在或执行失败后也必须出现救援 shell
./scripts/operator_smoke.py --arch rv --profile pre --smp 1 \
  --mode run --script /does/not/exist

# 多核首次 bring-up 较慢时提高整次测试预算
./scripts/operator_smoke.py --arch la --profile final --smp 8 --timeout 300
```

---

## GDB 卡死诊断

## 1. 支持的内核配置

GDB 调试同时支持 pre 和 final，并生成独立的、未裁剪的 ELF，不会覆盖普通 release
内核。


| 统一工具配置     | Make 运行目标            | 调试 ELF                       |
| ------------------ | -------------------------- | -------------------------------- |
| `rv-pre`         | `rv_pre_run-gdb`         | `kernel-rv-pre-gdb`            |
| `rv-final`       | `rv_final_run-gdb`       | `kernel-rv-final-gdb`          |
| `rv-final-log`   | `rv_final_run_log-gdb`   | `kernel-rv-final-log-gdb`      |
| `rv-final-debug` | `rv_final_debug_run-gdb` | `kernel-rv-final-debug-gdb`    |
| `rv-smp-test`    | `rv_final_smp_test-gdb`  | `kernel-rv-final-smp-test-gdb` |
| `la-pre`         | `la_pre_run-gdb`         | `kernel-la-pre-gdb`            |
| `la-final`       | `la_final_run-gdb`       | `kernel-la-final-gdb`          |
| `la-final-log`   | `la_final_run_log-gdb`   | `kernel-la-final-gdb`          |

调试 ELF 使用 Cargo `gdb` profile：保留 release 优化，同时启用完整 DWARF、符号表、
frame information 和 frame pointer。RISC-V、LoongArch 的 trap entry、`__switch`、
内核任务入口也包含 CFI。

## 2. 安装和检查依赖

Ubuntu 安装命令：

```bash
sudo apt install gdb-multiarch \
  binutils-riscv64-unknown-elf \
  binutils-loongarch64-linux-gnu \
  qemu-system-misc
```

工具不会自动执行 `sudo`。安装后运行：

```bash
./scripts/wateros_debug.py doctor
```

检查某一个架构和已经构建的 ELF：

```bash
./scripts/wateros_debug.py doctor \
  --arch rv \
  --elf ./kernel-rv-pre-gdb

./scripts/wateros_debug.py doctor \
  --arch la \
  --elf ./kernel-la-pre-gdb
```

`doctor` 会检查：

- `gdb-multiarch`、QEMU、Python、`readelf`；
- 对应架构的 `nm` 和 `addr2line`，找不到交叉工具时尝试宿主工具；
- ELF 的 DWARF、符号表和 frame information；
- trap、`__switch` 和任务入口是否有 CFI；
- 调试构建是否强制保留 frame pointer；
- ELF 内嵌的 WaterOS build ID。

任何必要项缺失时，`doctor` 返回非零状态。

## 3. 最简单的一键调试

推荐使用统一入口，它会依次构建调试 ELF、以 snapshot 模式启动 QEMU、保存串口，
然后自动监测卡死：

```bash
./scripts/wateros_debug.py run rv-pre --smp 8
./scripts/wateros_debug.py run rv-final --smp 8

./scripts/wateros_debug.py run la-pre --smp 8
./scripts/wateros_debug.py run la-final --smp 8
```

默认行为：

- 使用 `kernel-*-gdb`，不会覆盖普通内核；
- QEMU 启动后立即运行，同时开放 `127.0.0.1:1234`；
- 每秒采样一次，连续十次确认同一停滞原因后抓取完整现场；
- 使用 QEMU `-snapshot`，不写回基础磁盘镜像；
- 确认卡死并完成报告后保持 guest 暂停。

调整监测参数：

```bash
./scripts/wateros_debug.py run rv-pre \
  --smp 4 \
  --port 1235 \
  --interval 0.5 \
  --confirm 20 \
  --timeout 5
```

只有明确需要保存 guest 磁盘写入时才使用：

```bash
./scripts/wateros_debug.py run rv-final --smp 8 --write-disk
```

调试和回归测试通常不要使用 `--write-disk`。

## 4. 使用 Makefile 启动

所有真实运行目标都使用统一的 `-gdb` 后缀：

```bash
make rv_pre_run-gdb
make rv_final_run-gdb
make rv_final_run_log-gdb

make la_pre_run-gdb
make la_final_run-gdb
```

Make 模式默认传入 QEMU `-S`，即停在第一条指令等待调试器连接。让内核立即运行，
适合等待问题复现后再附加：

```bash
make rv_pre_run-gdb GDB_WAIT=0
make la_pre_run-gdb GDB_WAIT=0
```

修改端口或 CPU 数量：

```bash
WOS_SMP=4 make rv_pre_run-gdb GDB_WAIT=0 GDB_PORT=1235
```

GDB 模式默认使用 snapshot 磁盘。相关环境变量如下：


| 变量                             | 含义                                 |
| ---------------------------------- | -------------------------------------- |
| `WOS_SMP`                        | QEMU vCPU 数量，统一工具限制为`1..8` |
| `WOS_QEMU_GDB_PORT` / `GDB_PORT` | GDB Remote 端口，默认`1234`          |
| `WOS_QEMU_GDB_WAIT` / `GDB_WAIT` | `1` 表示传入 `-S`，`0` 表示立即运行  |
| `WOS_QEMU_SNAPSHOT`              | `1` 表示不写回磁盘，GDB 模式默认启用 |
| `WOS_KERNEL`                     | QEMU 实际加载的内核 ELF              |
| `WOS_SDCARD`                     | QEMU 使用的磁盘镜像                  |

## 5. 附加到已经运行的 QEMU

先启动 QEMU：

```bash
make rv_pre_run-gdb GDB_WAIT=0
```

然后在另一个终端选择以下方式之一。

### 5.1 自动监测

```bash
./scripts/wateros_debug.py watch \
  --arch rv \
  --elf ./kernel-rv-pre-gdb
```

指定其他端口：

```bash
./scripts/wateros_debug.py watch \
  --arch rv \
  --elf ./kernel-rv-pre-gdb \
  --port 1235
```

### 5.2 立即抓取一次完整现场

```bash
./scripts/wateros_debug.py snapshot \
  --arch rv \
  --elf ./kernel-rv-pre-gdb
```

默认抓取后恢复 guest。需要保留暂停状态：

```bash
./scripts/wateros_debug.py snapshot \
  --arch rv \
  --elf ./kernel-rv-pre-gdb \
  --leave-stopped
```

LoongArch 只需替换架构和 ELF：

```bash
./scripts/wateros_debug.py snapshot \
  --arch la \
  --elf ./kernel-la-pre-gdb
```

### 5.3 交互式 GDB

```bash
./scripts/wateros_debug.py gdb \
  --arch rv \
  --elf ./kernel-rv-pre-gdb
```

统一入口会自动连接 QEMU 并加载 WaterOS GDB 扩展。

## 6. WaterOS GDB 命令

交互 GDB 中可使用：

```gdb
wos-cpus
wos-tasks
wos-task 10
wos-events
wos-events 3
wos-locks
wos-snapshot
wos-snapshot 256
```

命令含义：


| 命令                  | 作用                                                     |
| ----------------------- | ---------------------------------------------------------- |
| `wos-cpus`            | 显示每 CPU online、当前任务、模式、队列、计数器和锁等待  |
| `wos-tasks`           | 显示各 CPU 当前任务的 kind/state/policy/nice/wait/aspace |
| `wos-task <id>`       | 查找指定任务的当前 CPU 状态和最近事件                    |
| `wos-events [cpu]`    | 显示全部或指定 CPU 的最近事件                            |
| `wos-locks`           | 显示锁 owner/waiter 和 wait-for 关系                     |
| `wos-snapshot [数量]` | 一次打印 CPU、事件及锁关系                               |

`wos-tasks` 第一版以每 CPU 当前任务和事件环为数据源，不枚举从未运行且已经离开
事件窗口的休眠任务。用户态首版只报告 user PC/SP、task、syscall 和 trap，不展开
用户 ELF 调用栈。

普通 GDB 命令仍然可用：

```gdb
info threads
thread apply all info all-registers
thread apply all bt full
x/12i $pc-16
x/32gx $sp
continue
detach
```

## 7. 如何理解 watch 输出

示例：

```text
[wos-debug] stable=1/10 reason=cpu6:lock pc=[0x..., 0x...]
```

- `stable=1/10`：同一停滞原因只连续出现了一次；达到 `10/10` 才确认卡死；
- `reason=cpu6:lock`：CPU 6 本次仍在等待同一个关键锁；
- 下一次恢复为 `0/10 reason=none` 表示只是正常的短暂锁竞争；
- `pc=[...]` 按 QEMU vCPU 顺序显示当前 PC。

轻量 watch 阶段不对每个 PC 反复执行 `addr2line`，因此只显示地址。确认卡死后的
`summary.txt` 和 `gdb.txt` 会包含函数、偏移、源码行、反汇编和调用栈。

手动解析单个内核地址：

```bash
./scripts/resolve_pc_symbol.py \
  --arch rv \
  --elf ./kernel-rv-pre-gdb \
  0x80248a18
```

`0x80201000` 一类地址通常位于内核 trap 入口。较小的地址，例如 `0x2084`，通常是
用户程序 PC，无法使用内核 ELF 解析。

自动判定按 CPU 组合检查：

- PC、SP 和事件 sequence 是否变化；
- timer、context switch、syscall、trap、IPI 是否推进；
- runqueue 非空且 `need_resched` 时是否长期不切换；
- 等待锁及已持有锁是否形成稳定 wait-for 链；
- 当前 CPU 是否正常 idle 且没有 runnable 任务。

健康 idle CPU 的 timer 不会掩盖另一个 CPU 的停滞。全部 CPU idle 且队列为空属于
正常静止；用户计算期间只要 PC、中断或事件仍有进展，也不会被判为卡死。

## 8. 诊断报告

完整报告保存到：

```text
debug-reports/<timestamp>-<arch>-<build-id>/
├── summary.txt
├── metadata.json
├── snapshot.json
├── events.json
├── gdb.txt
├── serial.log
├── serial-tail.txt
└── reproduce.txt
```

各文件用途：

- `summary.txt`：推断类型、CPU 表、PC/RA/SP/FP、函数和源码位置；
- `metadata.json`：架构、Git、ELF SHA-256、build ID 和 GDB 版本；
- `snapshot.json`：完整寄存器与 WaterOS 诊断状态；
- `events.json`：最近 task、syscall、trap、IPI、futex、TLB 和锁事件；
- `gdb.txt`：全寄存器、所有 CPU 的 `bt full`、反汇编和栈内存；
- `serial.log`：本次运行的完整串口；
- `serial-tail.txt`：串口末尾 300 行；
- `reproduce.txt`：重新连接当前现场的命令。

自动模式完成报告后保持 QEMU 暂停，并打印继续交互调试的命令。

## 9. 内核诊断区记录了什么

根 crate 的 `gdb-debug` feature 会启用
[`wateros-debug`](./components/wateros-debug/README.md)。诊断区包含：

- header：magic、ABI 版本、架构、build ID、CPU 容量和记录尺寸；
- 每 CPU 双缓冲状态：online、task、地址空间、模式、五类 runqueue、计数器、
  `need_resched`、调度原因、等待目标和关键锁；
- 每 CPU 256 项事件环：task enqueue/switch/block/wake/exit、syscall、trap、timer、
  IPI、futex、TLB shootdown 和锁事件；
- scheduler、process registry、futex registry、frame allocator、address-space/TLB、
  VFS、network 和 klog 的锁 owner/waiter；
- `dropped_updates`、`dropped_events` 和正在写入标记。

CPU 状态采用双缓冲发布。CPU 停在写入中间时，主机只读取上一份完整状态。事件记录
最后发布 sequence，主机忽略正在覆写或 sequence 不匹配的槽。

热路径不分配内存、不打印串口，也不获取诊断锁。关闭 `gdb-debug` 后记录接口为空操作，
普通 release 不导出诊断区。

## 10. Build ID 与符号文件

每次 GDB 构建都会生成独立 WaterOS build ID，并同时写入：

- 调试 ELF 导出符号；
- 版本化诊断 header；
- QEMU guest 内存。

工具在解释任何内核地址前进行三方校验。出现以下错误时：

```text
ELF/guest build ID mismatch
```

说明本地 ELF 不是 QEMU 当前加载的内核。停止 QEMU，重新执行对应的 Make 或 `run`
命令，不要用旧 ELF 强行解析地址。

## 11. 确定性故障注入

故障代码只在显式测试构建中存在：

```bash
make rv_pre_run-gdb GDB_FAULTS=1 GDB_WAIT=0
make la_pre_run-gdb GDB_FAULTS=1 GDB_WAIT=0
```

统一入口：

```bash
./scripts/wateros_debug.py run rv-pre --smp 2 --faults
```

连接 GDB，在 CPU 初始化完成后设置：

```gdb
set *(unsigned long *)&WATEROS_DEBUG_FAULT_MODE = 1
continue
```

故障模式：


| 值  | 故障                                             |
| ----- | -------------------------------------------------- |
| `0` | 关闭故障注入                                     |
| `1` | CPU 0 在 timer trap 中进入固定死循环             |
| `2` | CPU 0/1 构造 ABBA 锁死，要求至少两个 CPU         |
| `3` | CPU 0 关闭本地 timer 并停止调度                  |
| `4` | CPU 0 的 timer 继续，但故意不执行 scheduler tick |

普通 `gdb-debug` 和 release 构建不包含 `WATEROS_DEBUG_FAULT_MODE`。

## 12. 测试调试工具

运行 Python 单元测试：

```bash
python3 -m unittest discover -s scripts/tests -v
```

检查双架构普通构建：

```bash
make rv_check
make la_check
```

检查双架构 GDB 构建：

```bash
make rv_check GDB_BUILD=1
make la_check GDB_BUILD=1
```

检查故障注入构建：

```bash
make rv_check GDB_BUILD=1 GDB_FAULTS=1
make la_check GDB_BUILD=1 GDB_FAULTS=1
```

构建独立调试 ELF：

```bash
make kernel-rv-pre GDB_BUILD=1
make kernel-la-pre GDB_BUILD=1
```

## 13. 常见问题

### QEMU 一启动就没有输出

`make ...-gdb` 默认 `GDB_WAIT=1`，QEMU 停在第一条指令。连接 GDB 后执行
`continue`，或者用 `GDB_WAIT=0` 重新启动。

### 端口被占用

启动端和调试端必须使用相同的新端口：

```bash
make rv_pre_run-gdb GDB_WAIT=0 GDB_PORT=1235

./scripts/wateros_debug.py watch \
  --arch rv \
  --elf ./kernel-rv-pre-gdb \
  --port 1235
```

### watch 没有立即判定锁死

一次锁等待通常只是正常竞争。工具要求同一原因连续达到确认阈值，避免把短临界区误报
为死锁。可通过 `--confirm` 修改阈值，但不建议在压力测试中设置得过低。

### 报告中的事件有 dropped 数量

事件环固定为每 CPU 256 项。高频压力测试会覆盖旧记录，`dropped_events` 表示已经
回卷的数量。卡死后事件停止推进，报告仍会保留停滞前最后 256 项。CPU 表中
`DROP(U/E)` 分别表示状态更新丢弃数和事件覆盖数，末尾 `*` 表示采样时 CPU 正在
发布新状态。

### 如何恢复一个被 watch 留在暂停状态的 guest

重新连接：

```bash
./scripts/wateros_debug.py gdb \
  --arch rv \
  --elf ./kernel-rv-pre-gdb
```

然后执行：

```gdb
continue
```

完整的底层设计和历史排障案例另见
[`docs/debugging/GDB_STALL_DEBUG.md`](../docs/debugging/GDB_STALL_DEBUG.md)。

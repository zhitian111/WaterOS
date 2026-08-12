# WaterOS RISC-V / LoongArch 卡死诊断：统一 GDB 工具

[项目首页](../../README.md) · [工具总览](./README.md) · [内核工程](../../os/README.md)

## 0. 推荐入口

WaterOS 使用 `os/Makefile` 作为操作者入口，并由
`os/scripts/debug/wateros_debug.py` 完成构建、启动、监测和归档现场。
`gdb_remote_snapshot.py` 只保留 Remote packet/register/memory 底层模块，不再提供
独立 CLI；本文后半部分的旧命令仅作为历史排障案例，新的脚本或自动化不得调用它。

Ubuntu 首次使用先安装依赖并执行预检：

```bash
sudo apt install gdb-multiarch binutils-riscv64-unknown-elf \
  binutils-loongarch64-linux-gnu qemu-system-misc
cd os
make doctor
```

最常用命令：

```bash
# 自动挡：启动 QEMU 并 watch
make debug ARCH=rv PROFILE=final SMP=8
make debug ARCH=la PROFILE=final SMP=8

# 手动挡：终端一
make debug-server ARCH=rv PROFILE=final SMP=8

# 手动挡：终端二
make gdb
make snapshot
make watch
```

`make debug` 默认以 QEMU snapshot 模式运行，不写回基础磁盘。
只有明确需要保存 guest 写入时才传 `WRITE_DISK=1`。监测默认每秒
采样，按 CPU 组合检查 PC/SP、timer、
context switch、syscall、trap、IPI、事件 sequence、runqueue 与锁等待；同一停滞原因
连续出现十次后才抓取完整现场。可用 `--interval`、`--confirm` 调整。健康 idle CPU
的 timer 不会掩盖另一个 CPU 的锁死，长时间用户计算只要 PC 或中断/事件仍推进也
不会被当作全局卡死。

确认停滞后 QEMU 会保持暂停，报告位于：

```text
os/debug-reports/<timestamp>-<arch>-<build-id>/
  summary.txt       人工可读结论、CPU 表和 PC/SP/RA/FP
  metadata.json     Git、ELF SHA-256、build ID、GDB 版本
  snapshot.json     结构化 CPU/寄存器/调度状态
  events.json       最近调度、trap、syscall、IPI、futex 与锁事件
  gdb.txt           全寄存器、反汇编、栈内存与 thread apply all bt full
  serial.log        本次运行串口
  serial-tail.txt   串口末尾 300 行
  reproduce.txt     重新连接现场的命令
```

交互 GDB 可使用 `wos-cpus`、`wos-tasks`、`wos-task <id>`、
`wos-events [cpu]`、`wos-locks` 和 `wos-snapshot`。诊断区通过 build ID 校验本地
ELF 与 guest；不匹配时工具拒绝继续符号化，避免给出看似合理但完全错误的函数名。
`wos-tasks` 第一版列出各 CPU 当前任务及其 state/policy/nice/wait；`wos-task <id>`
再结合最近事件环定位该任务。尚未运行且不在最近事件窗口内的任务不会被枚举，用户态
也只报告保存的 PC/SP、trap 与 syscall，不尝试展开用户 ELF 调用栈。

### 0.1 确定性故障测试

故障代码只存在于显式调试构建：

```bash
make debug-server ARCH=rv PROFILE=pre SMP=2 FAULTS=1 START_PAUSED=0
# LoongArch 使用 ARCH=la；第二个终端执行 make gdb
```

连接 GDB 后，在系统完成 AP online 后写入模式；下一次 timer trap 触发：

```gdb
set *(unsigned long *)&WATEROS_DEBUG_FAULT_MODE = 1  # 固定 PC 死循环
set *(unsigned long *)&WATEROS_DEBUG_FAULT_MODE = 2  # CPU 0/1 ABBA
set *(unsigned long *)&WATEROS_DEBUG_FAULT_MODE = 3  # 停止本 CPU timer
set *(unsigned long *)&WATEROS_DEBUG_FAULT_MODE = 4  # timer 继续但不调度
continue
```

模式 2 至少需要 `WOS_SMP=2`；模式 3/4 应在目标 CPU 有对应 idle/runnable 条件时
使用。普通 `make ...-gdb` 和所有 release 内核都不包含这些符号。

## 1. 原理与手工流程

本文记录一次真实的 WaterOS SMP 卡死排查方法。目标不是讲完 GDB，而是让第一次使用
GDB 的人能够完成下面这条链路：

1. 用带符号的内核启动 QEMU；
2. 卡住时暂停全部 hart；
3. 查看每个 hart 的 `pc`、`ra`、`sp`；
4. 把地址还原成 Rust 函数；
5. 反汇编锁的自旋循环；
6. 根据多个 hart 的位置判断死锁或锁顺序反转。

### 1.1 启动调试内核

普通运行不包含 stall watchdog，也不开放 GDB 端口：

```bash
make rv_final_run
```

启用 Cargo feature `stall-debug` 并保存串口日志，但不开放 GDB 端口：

```bash
make rv_final_run_log
```

启用 `stall-debug`，同时在本机 `127.0.0.1:1234` 开放 QEMU GDB Remote
端口，并在第一条指令前暂停：

```bash
make rv_final_run_log-gdb
```

如果要先运行到疑似卡死的位置，再连接调试器：

```bash
make rv_final_run_log-gdb GDB_WAIT=0
```

`-gdb` 是所有真实运行目标共用的后缀，对应参数如下：

| Make 参数 | 作用 |
|------|------|
| Cargo feature `stall-debug` | 编译 syscall/timer 原子采样和低频 watchdog；默认关闭 |
| `GDB_WAIT=1` | 默认值；传入 QEMU `-S`，连接并执行 `continue` 前不运行 |
| `GDB_WAIT=0` | 开放端口后立即运行，适合采集运行中卡死现场 |
| `GDB_PORT=1235` | 修改监听端口，默认是 1234 |

所有 `-gdb` 目标使用独立 Cargo `gdb` profile，并生成 `kernel-*-gdb`；普通
`kernel-rv-pre`、`kernel-rv-final`、`kernel-la-*` 不会被调试构建覆盖。

也可以直接调用脚本：

```bash
WOS_KERNEL=./kernel-rv-final-log \
WOS_QEMU_GDB=1 \
WOS_QEMU_GDB_PORT=1234 \
bash ./scripts/run/rv_final_run.sh
```

LoongArch 对应目标如下：

```bash
# 初赛镜像：暂停并等待连接
make la_pre_run-gdb

# 初赛镜像：立即运行并开放端口
make la_pre_run-gdb GDB_WAIT=0

# 决赛镜像：暂停并等待连接
make la_final_run-gdb
```

`stall-debug` 只在连续多个采样周期没有 syscall 进展时打印：

- 各 CPU 的 timer 是否仍在到达；
- 当前是否卡在 `brk`、`munmap` 或 `mprotect`；
- 非 idle 任务的 Running/Ready/Blocking 状态；
- `WaitQueue` 的来源，例如 `futex`、`pipe-read`、`eventfd`；
- 最近的 futex wait/wake 摘要。

出现一次 `no syscall progress` 不等于死锁。编译器长时间做纯计算也可能没有
syscall，必须结合任务状态和 GDB 采样判断。

## 2. 准备符号文件

调试器打开的文件必须与 QEMU 正在运行的内核完全一致：

```bash
file kernel-rv-final-log
```

预期包含：

```text
ELF 64-bit ... RISC-V ... statically linked, not stripped
```

`not stripped` 表示函数符号仍在。若 QEMU 使用了旧内核，而调试器打开了新内核，
地址解析结果会完全错误。每次改代码后重新执行对应的 `make` 目标。

## 3. macOS：RISC-V 使用 LLDB

macOS 通常没有 `riscv64-unknown-elf-gdb`，但系统 LLDB 可以使用同一个 GDB
Remote 协议。

先在终端 A 启动：

```bash
make rv_final_run_log-gdb GDB_WAIT=0
```

程序疑似卡死时，在终端 B 执行：

```bash
lldb ./kernel-rv-final-log
```

进入 LLDB 后连接：

```text
(lldb) gdb-remote 127.0.0.1:1234
```

连接会暂停 guest。QEMU 的每个 hart 会显示为一个调试器 thread：

```text
(lldb) thread list
```

如果只想快速保存一份非交互快照，可以另开终端执行：

```bash
lldb -b ./kernel-rv-final-log \
  -o 'gdb-remote 127.0.0.1:1234' \
  -o 'thread list' \
  -o 'thread backtrace all' \
  -o 'register read pc ra sp fp'
```

批处理结束后如果 guest 仍处于暂停状态，重新交互连接并执行 `continue`，或重启
QEMU。

先查看全部 hart 的调用栈：

```text
(lldb) thread backtrace all
```

release 内核或手写上下文切换可能导致栈展开失败。此时不要停在 `bt`，直接读取
寄存器。

选择一个 hart：

```text
(lldb) thread select 3
```

读取最关键的寄存器：

```text
(lldb) register read pc ra sp fp
```

也可以尝试：

```text
(lldb) register read --all
```

QEMU GDB stub 不一定暴露 `satp`、`scause`、`sepc`、`stval` 等 CSR；提示寄存器
不存在时，以内核 trap 日志和 trap frame 为准。

### 3.1 LoongArch 不要使用 Apple LLDB

当前 Apple LLDB 可以识别 `kernel-la-pre` 的 LoongArch ELF 类型，但无法正确解析
QEMU 返回的 LA 寄存器描述。典型现象是 `thread list` 可见 8 个 CPU，
`register read pc ra sp` 却失败或显示 `0xffffffffffffffff`。断开一个由 `-S`
暂停的连接还可能使 QEMU 退出。

有支持 LA 的 GNU GDB 时，终端 A、B 分别运行：

```bash
# 终端 A
make la_pre_run-gdb

# 终端 B
loongarch64-linux-gnu-gdb ./kernel-la-pre \
  -ex 'set architecture loongarch64' \
  -ex 'target remote 127.0.0.1:1234'
```

使用 multiarch GDB 或非默认端口：

```bash
gdb-multiarch ./kernel-la-pre \
  -ex 'set architecture loongarch64' \
  -ex 'target remote 127.0.0.1:1235'
```

旧版本在 macOS 没有 LA GDB 时曾使用只读快照客户端：

```bash
make la_gdb_snapshot
```

当前该 Make 目标已经改为统一 `wateros_debug.py snapshot` 的兼容别名，并强制依赖
`gdb-multiarch`。底层 [`gdb_remote_snapshot.py`](../../os/scripts/debug/gdb_remote_snapshot.py)
只负责 Remote packet、寄存器描述和内存读取，不再接受命令行参数。

### 3.2 历史快照客户端（CLI 已移除）

> 本节记录旧实现，命令已经不可执行。现在统一使用
> `wateros_debug.py snapshot --arch la --elf ./kernel-la-pre-gdb`；该入口强制
> `gdb-multiarch`、校验 build ID 并生成完整报告包。

#### 最短操作流程

先进入 `os` 目录，在终端 A 启动带 GDB Remote 端口的 QEMU：

```bash
cd /Users/x/code/WaterOS/os
make la_pre_run-gdb GDB_WAIT=0
```

等系统运行到疑似卡死的位置后，在终端 B 采集快照：

```bash
cd /Users/x/code/WaterOS/os
make la_gdb_snapshot
```

决赛内核需要让快照脚本读取与 QEMU 完全相同的 ELF：

```bash
# 终端 A
make la_final_run-gdb GDB_WAIT=0

# 终端 B
make la_gdb_snapshot LA_GDB_ELF=./kernel-la-final
```

如果 1234 端口已占用，两端必须使用同一个新端口：

```bash
# 终端 A
make la_pre_run-gdb GDB_WAIT=0 GDB_PORT=1235

# 终端 B
make la_gdb_snapshot GDB_PORT=1235
```

旧版 `make la_gdb_snapshot` 曾等价于（仅供理解历史报告）：

```bash
python3 ./scripts/debug/gdb_remote_snapshot.py \
  --arch la \
  --elf ./kernel-la-pre \
  --host 127.0.0.1 \
  --port 1234
```

旧版底层脚本也曾直接支持 RISC-V：

```bash
python3 ./scripts/debug/gdb_remote_snapshot.py \
  --arch rv \
  --elf ./kernel-rv-final-log \
  --port 1234
```

#### 参数

| 参数 | 含义 |
|------|------|
| `--arch {la,rv}` | 必填，选择 LoongArch 或 RISC-V 寄存器和地址规则 |
| `--elf PATH` | 必填，用于解析地址的未剥离内核 ELF，必须与 QEMU 中运行的文件一致 |
| `--host HOST` | GDB Remote 地址，默认 `127.0.0.1` |
| `--port PORT` | GDB Remote 端口，默认 `1234` |
| `--timeout SECONDS` | 建连和读取超时，默认 5 秒 |
| `--stack-words N` | 从每个 CPU 的 SP 开始扫描多少个 64 位机器字，默认 64；设为 0 可关闭 |
| `--leave-stopped` | 采集后不 detach，让 QEMU 保持暂停 |

通常不要使用 `--leave-stopped`。默认模式采集结束会显示：

```text
[gdb-snapshot] detached; guest resumed
```

此时 guest 已恢复运行。如果使用了 `--leave-stopped`，脚本会显示：

```text
[gdb-snapshot] guest remains stopped
```

此时必须用 GNU GDB 连接并执行 `continue`，或者重新连接后 detach，系统才会继续
运行。

#### 如何阅读输出

一次正常的 LA 多核快照类似：

```text
[gdb-snapshot] guest stopped: T...
[gdb-snapshot] 8 CPU thread(s)
cpu=0 thread=1 pc=0x0000000090... ra=0x... sp=0x... fp=0x... badv=0x...
  0x0000000090...  function_name+offset
  stack+0x030: 0x0000000090...  caller_name+offset
```

- `CPU thread(s)` 应与 QEMU 的 `-smp` 数量一致；
- `pc` 是该 CPU 当前执行地址，`ra` 是返回地址，`sp`/`fp` 是栈指针/帧指针；
- LA 目标若由 QEMU 暴露相应 CSR，还会打印 `badv`、`era`、`estat`、`prmd`
  等诊断寄存器；
- 紧随寄存器行的符号是 `pc` 对应的 Rust/汇编函数；
- `stack+0x...` 是在栈中找到的疑似内核返回地址，只是调用链线索，不保证每一项
  都是有效栈帧；
- 若 CPU 位于 `fatal_kernel_trap`，脚本还会尝试打印 `raw_cause`、
  `trapped_pc`、`fault_addr` 和异常发生位置；
- 地址显示为 `firmware/out of kernel ELF` 时，该 CPU 仍在 QEMU 固件范围；
  `<unknown>` 则表示地址不在当前 ELF 的已知符号范围，首先检查 ELF 是否匹配。

判断是否真的卡死时应间隔数秒采集两次。如果同一批 CPU 的 `pc` 始终停在同一
自旋锁，而且 syscall/任务状态没有推进，才更像内核死锁；两次快照地址持续变化
通常表示任务仍在执行。

#### 依赖与失败处理

脚本本身只依赖 Python 3 标准库和仓库内的 `scripts/debug/symbol_index.py`，不要求
Python GDB 模块。符号解析还需要 `nm`：

- LA 优先使用 `loongarch64-linux-gnu-nm`；
- RISC-V 优先使用 `riscv64-elf-nm`；
- 找不到交叉工具时，会尝试使用当前 Rust toolchain 自带的 `llvm-nm`。

常见错误：

- `Connection refused`：QEMU 没有用 `*_gdb_run`/`*_gdb_wait` 启动，端口写错，
  或 QEMU 已退出；
- `ELF not found`：先构建对应内核，或通过 `LA_GDB_ELF`/`--elf` 指定正确文件；
- `failed to run nm`：安装对应 binutils，或通过 rustup 安装包含 LLVM tools 的
  Rust 组件；
- 地址都解析成不相关函数：QEMU 与脚本使用的不是同一次构建产生的 ELF；
- 连接后一直没有串口输出：检查是否使用了 `--leave-stopped`，或 QEMU 是否由
  `*_gdb_wait` 的 `-S` 停在启动阶段。

## 4. 从地址查函数

假设某个 hart 的寄存器为：

```text
pc = 0x802e1022
ra = 0x802e0f9a
sp = 0x803a5d50
```

查询 `pc` 属于哪个符号：

```text
(lldb) image lookup --address 0x802e1022
```

查询返回地址：

```text
(lldb) image lookup --address 0x802e0f9a
```

`ra` 指向调用返回后的下一条指令，不一定正好落在 `call` 上。RISC-V 启用了压缩
指令，调用指令可能是 2 或 4 字节，因此不要固定只查 `ra - 4`。更可靠的方法是
反汇编 `ra` 前后的一小段：

```text
(lldb) disassemble --start-address 0x802e0f90 --count 12
```

反汇编当前 `pc` 周围：

```text
(lldb) disassemble --start-address 0x802e1000 --count 32
```

如果 LLDB 不能给出源码行，仍可用仓库脚本查询：

```bash
make rv_symbol_at ADDR=0x802e1022
```

或者使用 Rust 工具链的 `nm`：

```bash
rust-nm -n kernel-rv-final-log | rg 'with_scheduler|copy_from_user'
```

## 5. 如何识别自旋锁

自旋锁常表现为一个很短的循环：

```text
load/amo  lock_word
branch    lock_not_available
jump      loop
```

判断步骤：

1. `thread list` 中多个 hart 的 `pc` 落在同一个很短的地址范围；
2. 反汇编该范围，确认存在反复读取锁字并向后跳转的循环；
3. `continue` 运行一小段时间，再用 `Ctrl-C` 暂停；
4. 再读一次各 hart 的 `pc`；如果仍落在同一个循环，说明它们持续等待该锁。

LLDB 中继续执行：

```text
(lldb) continue
```

需要重新暂停时在 LLDB 终端按 `Ctrl-C`。

不要在 QEMU `-nographic` 的串口终端使用普通 `Ctrl-C` 判断内核状态。退出 QEMU
应使用：

```text
Ctrl-A X
```

即先按 `Ctrl-A`，松开后再按 `X`。

## 6. 本次 scheduler/futex 死锁是怎样找到的

当时串口长期没有 syscall 进展。连接 LLDB 后看到：

- 7 个 hart 的 `pc` 都在
  `wateros_task_scheduler_impl_multi_class::with_scheduler` 的锁循环；
- 剩余 1 个 hart 在 `Sv39UserMemoryOps::copy_from_user` 内等待地址空间锁；
- 该 hart 的 `ra` 和附近反汇编继续向上指向：
  `wait_current_while -> futex::wait_while`。

关系可以写成：

```text
hart A:
  持有 scheduler lock
    -> futex wait condition
      -> copy_from_user
        -> 等 address-space lock

hart B:
  持有/操作 address-space lock
    -> 需要 scheduler lock

其他 hart:
  全部等待 scheduler lock
```

这就是锁顺序反转。关键证据不是“某个 PC 看起来不动”，而是：

1. 多个 hart 聚集在 scheduler lock；
2. 唯一不在该锁上的 hart 位于用户内存复制路径；
3. 它的返回地址表明用户内存复制发生在 scheduler 临界区内；
4. 两边分别等待对方所需的锁。

修复原则是：futex 条件中的用户地址访问必须发生在 scheduler 锁外。进入
scheduler 临界区后只比较内核原子 wake sequence，避免再次取得地址空间锁。

### 6.1 本次 LoongArch 停滞是怎样继续缩小的

串口停在 `kernel runner enqueued` 后，`make la_gdb_snapshot` 首次得到：

```text
cpu=0 pc=0x900021b4  write_registered_uart_console
cpu=1..7 pc=0x1c000050  firmware/out of kernel ELF
```

反汇编 `0x900021b4` 后确认它是字符设备 `spin::Mutex` 的短自旋循环。修正
UART 日志重入后再次采样，CPU0 落到 `console_write_fmt` 的控制台锁；栈扫描同时
发现 `fatal_kernel_trap`，说明真正的异常正被日志路径的二次死锁掩盖。控制台因此
改为：

- 内核日志保持使用板级 raw UART，不再反向进入 `/dev/console` 字符设备锁；
- 控制台持锁期间屏蔽本 CPU 中断，重入/争用时不等待而直接使用 raw UART；
- 日志写失败采用 best-effort，不因 `unwrap` 触发第二次 panic。

解除日志死锁后，稳定的采样点变为：

```text
cpu=0 pc=__tlb_refill badv=0x12032f04c
```

对照 Linux 官方 `arch/loongarch/mm/tlb.c` 与 `tlbex.S` 后确认，WaterOS
的三级页表 walker 配置错了一层：

- WaterOS 把 VA bit 30 的 PGD 索引写到了 `PWCL.Dir2`，并将 `PWCH` 置 0；
- refill 汇编相应使用 `lddir ..., 2`；
- Linux 的三级配置把 PTE/PMD 写入 `PWCL`，把 bit 30、宽 9 的 PGD 写入
  `PWCH`，refill 顺序为 `lddir ..., 3` 再 `lddir ..., 1`。

因此硬件 walker 并非只在某个 lazy VMA 上失败，而是在用户态发生 TLB miss 时
按错误层级索引页表，最终不断重新进入 `__tlb_refill`。修复是将 PGD 配置移到
`PWCH`，同时把汇编首个目录层级改为 3。

还有一个实现差异需要配套处理：Linux 的空 PGD/PMD 项指向共享的 invalid
lower-level table，而 WaterOS 的新页表目录直接清零。为避免硬件 refill walker
沿空目录走到物理地址 0，WaterOS 在登记 lazy VMA 时预建其目录路径，但叶 PTE
仍保持无效，因此实际数据页仍由普通 page-fault 路径按需分配。

对应参考源码：

- [Linux LoongArch `tlb.c`](https://github.com/torvalds/linux/blob/master/arch/loongarch/mm/tlb.c)
  展示 `PWCL`/`PWCH` 的三级、四级页表配置；
- [Linux LoongArch `tlbex.S`](https://github.com/torvalds/linux/blob/master/arch/loongarch/mm/tlbex.S)
  展示 refill 路径所用的 `lddir` 层级；
- [Linux LoongArch `pgtable.h`](https://github.com/torvalds/linux/blob/master/arch/loongarch/include/asm/pgtable.h)
  展示空目录项与 invalid lower-level table 的关系。

当时 vCPU 1～7 位于固件且 `ra/sp=0`，说明它们没有进入 WaterOS，并不是
scheduler idle。后续补齐的 LA SMP 启动流程使用了与 Linux 相同的 mailbox
协议：BSP 向目标核 mailbox 0 写入平台 `_start`，再发送 boot IPI；AP 从
`CSR.CPUID`（CSR `0x20`）读取核号并选择独立 boot stack。

这里曾有一个很隐蔽的错误：入口和 `current_cpu_id()` 同时把 CPUID CSR 错写成
`0x10`。两边读到的都是 0，所以初始化校验会错误通过，但所有 AP 实际共用 CPU0
启动栈，随后表现为随机非法指令和嵌套 page fault。修正为 `0x20` 后，GDB 快照
应看到 8 个 vCPU 均位于 WaterOS 符号范围，且内核栈地址不再全部落在 CPU0 的
boot stack。

LA 多核正常启动还依赖两项配套逻辑：

- 运行期 IOCSR IPI 必须携带一个非零硬件 action bit，不能只写目标 CPU 编号；
- trap 解码必须先识别 `ESTAT.IS.IPI`，清除本地 IPI 状态后再处理 WaterOS
  保存的软件 reschedule/TLB shootdown 原因。

在 cyclictest + 400-worker hackbench 中，多数 CPU 可能被快照在
`__tlb_refill`。这不必然表示 refill 入口死循环：应间隔采样并检查其他 CPU 是否
在 signal、pipe、syscall 和 scheduler 路径间变化。一次实测中 `STRESS_P8` 停在
T1 后仍持续占用约 6 个宿主核，最终约 140 秒完成全部 T2～T7 并成功回收
hackbench。

当时还存在两处可消除的冗余 TLB 刷新：

- 公共 trap handler 在每次用户 trap 入口切换到 kernel PGDL；
- LA 返回用户态时无条件重写 PGDL 并执行全量 `invtlb`。

LA 已配置仅供 PLV0 使用的 DMW0，内核 RAM/MMIO 不依赖 kernel PGDL，因此修复后
用户 trap 期间保留当前用户 PGDL，返回时也只在目标 PGDL 真正变化时刷新。RISC-V
仍保持进入 kernel `satp` 的原有语义。不同进程切换目前仍必须刷新，因为 LA
地址空间尚未分配硬件 ASID；不能简单删除这次刷新，否则不同进程的同一虚拟地址
可能复用错误的旧 TLB 映射。

参考实现可对照
[Linux LoongArch `smp.c`](https://github.com/torvalds/linux/blob/master/arch/loongarch/kernel/smp.c)
和 [QEMU LoongArch `boot.c`](https://github.com/qemu/qemu/blob/master/hw/loongarch/boot.c)。

## 7. 如何区分三种“卡住”

### 7.1 全局锁死

典型表现：

- 多数 hart 在同一个 `with_scheduler` 或其他锁循环；
- syscall 和任务状态长期不变；
- timer 可能仍进入 trap，但无法完成调度工作。

应重点检查唯一不在该锁循环中的 hart，因为它通常是锁持有者或依赖链的另一端。

### 7.2 所有 hart 都在 idle

典型表现：

- 所有 hart 的 `pc` 都在 `__wateros_idle_task_runtime_main`；
- stall 快照没有 Ready/Running 用户任务；
- 用户任务全部是 `Blocking(WaitQueue(...))`、`ChildExit` 等状态。

这不是锁自旋，而是唤醒丢失、资源未关闭、pipe EOF 未产生，或者用户程序自身等待
关系有问题。查看日志中的 `wait=Some("pipe-read")`、`wait=Some("futex")` 等标签，
沿对应资源的 wake/close 路径检查。

### 7.3 用户任务仍在运行，只是很久没有输出

典型表现：

- 至少一个用户任务为 Running 或 Ready；
- 两次 GDB 采样的 `pc` 会变化；
- syscall 总数随后继续增长。

编译器进行纯计算时会出现这种情况。它不是内核卡死，不应因为一次 500 tick 告警
就修改调度器。

## 8. GNU GDB 等价命令

如果已经安装支持目标架构的 GNU GDB：

```bash
riscv64-unknown-elf-gdb ./kernel-rv-final-log
```

常用命令：

```text
(gdb) target remote 127.0.0.1:1234
(gdb) info threads
(gdb) thread apply all bt
(gdb) thread 3
(gdb) info registers pc ra sp s0
(gdb) info symbol 0x802e1022
(gdb) x/24i 0x802e1000
(gdb) x/16gx $sp
(gdb) continue
```

暂停执行使用 `Ctrl-C`。断点示例：

```text
(gdb) break wateros_task_scheduler_impl_multi_class::with_scheduler
(gdb) continue
```

Rust 泛型符号可能很长，直接按完整函数名下断点不方便。可以先：

```text
(gdb) info functions with_scheduler
```

再复制具体符号名，或者先使用地址断点：

```text
(gdb) break *0x802e1022
```

LoongArch 使用对应交叉 GDB 连接：

```bash
loongarch64-linux-gnu-gdb ./kernel-la-pre \
  -ex 'set architecture loongarch64' \
  -ex 'target remote 127.0.0.1:1234'
```

连接后常用寄存器名仍是 `pc`、`ra`、`sp`；部分 GDB 将 LA 通用寄存器显示为
`r1`（RA）、`r3`（SP）和 `r22`（FP）。

## 9. 栈无法展开时的最低限度方法

即使 `bt` 完全失败，仍可以按下面顺序工作：

1. 记录每个 hart 的 `pc`、`ra`、`sp`；
2. 对每个 `pc` 执行 `image lookup --address`；
3. 反汇编 `pc` 前后 20～40 条指令；
4. 反汇编 `ra` 前后，定位调用点；
5. 读取栈顶若干机器字，寻找落在内核文本区的地址：

```text
(lldb) memory read --format x --size 8 --count 32 $sp
```

6. 对疑似代码地址逐个执行 `image lookup --address ADDRESS`；
7. 运行一小段后再次采样，区分固定自旋与正常推进。

WaterOS RISC-V 内核通常从 `0x80200000` 附近开始，栈中的相近地址可优先尝试，但
最终必须以当前 ELF 的符号查询结果为准。

WaterOS LoongArch 内核文本通常从 `0x90000000` 开始；`0x1c000000` 附近属于
QEMU LA 固件，不应使用内核 ELF 强行解析。

## 10. 常见问题

### 端口 1234 已被占用

```bash
lsof -nP -iTCP:1234 -sTCP:LISTEN
```

换端口启动：

```bash
make rv_final_run_log-gdb GDB_WAIT=0 GDB_PORT=1235
```

LA 同样适用：

```bash
make la_pre_run-gdb GDB_WAIT=0 GDB_PORT=1235
make la_gdb_snapshot GDB_PORT=1235
```

### 地址全部解析成错误函数

几乎总是 QEMU 内核与 LLDB/GDB 打开的 ELF 不一致。停止 QEMU，重新构建并确认
`WOS_KERNEL` 指向同一个文件。

### 连接后系统不再输出

调试器连接或命中断点时 guest 是暂停的。执行 `continue` 才会恢复。

### `*-gdb` 启动后没有任何内核输出

这是预期行为：`GDB_WAIT=1` 是默认值，QEMU 使用了 `-S`。连接调试器后执行
`continue`；若不希望启动时暂停，追加 `GDB_WAIT=0`。

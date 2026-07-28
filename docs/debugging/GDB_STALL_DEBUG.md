# WaterOS RISC-V / LoongArch 卡死诊断：stall-debug 与 GDB

本文记录一次真实的 WaterOS SMP 卡死排查方法。目标不是讲完 GDB，而是让第一次使用
GDB 的人能够完成下面这条链路：

1. 用带符号的内核启动 QEMU；
2. 卡住时暂停全部 hart；
3. 查看每个 hart 的 `pc`、`ra`、`sp`；
4. 把地址还原成 Rust 函数；
5. 反汇编锁的自旋循环；
6. 根据多个 hart 的位置判断死锁或锁顺序反转。

## 1. 启动调试内核

普通运行不包含 stall watchdog，也不开放 GDB 端口：

```bash
make rv_final_run
```

启用 Cargo feature `stall-debug` 并保存串口日志，但不开放 GDB 端口：

```bash
make rv_final_run_log
```

启用 `stall-debug`，同时在本机 `127.0.0.1:1234` 开放 QEMU GDB Remote
端口：

```bash
make rv_final_gdb_run
```

如果需要从第一条内核指令开始调试，可以让 QEMU 启动后立即暂停：

```bash
make rv_final_gdb_wait
```

对应的底层开关如下：

| 开关 | 作用 |
|------|------|
| Cargo feature `stall-debug` | 编译 syscall/timer 原子采样和低频 watchdog；默认关闭 |
| `WOS_QEMU_GDB=1` | 开放 GDB 端口，guest 仍正常运行 |
| `WOS_QEMU_GDB_WAIT=1` | 开放 GDB 端口并传入 QEMU `-S`，连接前不运行 |
| `WOS_QEMU_GDB_PORT=1235` | 修改监听端口，默认是 1234 |

也可以直接调用脚本：

```bash
WOS_KERNEL=./kernel-rv-final-log \
WOS_QEMU_GDB=1 \
WOS_QEMU_GDB_PORT=1234 \
bash ./scripts/rv_final_run.sh
```

LoongArch 对应目标如下：

```bash
# 初赛镜像：运行并开放 1234 端口
make la_pre_gdb_run

# 初赛镜像：停在第一条指令，连接后才运行
make la_pre_gdb_wait

# 决赛镜像
make la_final_gdb_run
make la_final_gdb_wait
```

`make la_gdb_run` 和 `make la_gdb_wait` 当前是初赛目标的简写。

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
make rv_final_gdb_run
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
make la_pre_gdb_run

# 终端 B
make la_gdb
```

默认客户端是 `loongarch64-linux-gnu-gdb`。使用 multiarch GDB 或非默认端口：

```bash
make la_gdb LA_GDB=gdb-multiarch GDB_PORT=1235
```

macOS 没有 LA GDB 时，使用仓库内的只读快照客户端：

```bash
make la_gdb_snapshot
```

它通过 QEMU GDB Remote 协议暂停 guest，读取每个 vCPU 的
`pc`、`ra`、`sp`、`fp`，扫描栈中的内核代码地址，解析符号后自动 detach 并恢复
guest。调试决赛 ELF 或自定义端口：

```bash
make la_gdb_snapshot LA_GDB_ELF=./kernel-la-final GDB_PORT=1235
```

快照客户端不提供单步和断点；需要这些能力时仍应安装 GNU GDB。

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

另外，vCPU 1～7 位于固件且 `ra/sp=0` 表示它们没有进入 WaterOS，并不是
scheduler idle。当前 LA 平台尚未实现从 BSP 主动启动 AP；调度行为应先按单核
解释，不能仅因 QEMU 使用了 `-smp 8` 就假定 8 核已上线。

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

LoongArch 可直接通过 Makefile 连接：

```bash
make la_gdb
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
WOS_QEMU_GDB_PORT=1235 make rv_final_gdb_run
```

LA 同样适用：

```bash
WOS_QEMU_GDB_PORT=1235 make la_pre_gdb_run
make la_gdb_snapshot GDB_PORT=1235
```

### 地址全部解析成错误函数

几乎总是 QEMU 内核与 LLDB/GDB 打开的 ELF 不一致。停止 QEMU，重新构建并确认
`WOS_KERNEL` 指向同一个文件。

### 连接后系统不再输出

调试器连接或命中断点时 guest 是暂停的。执行 `continue` 才会恢复。

### `rv_final_gdb_wait` 启动后没有任何内核输出

这是预期行为：QEMU 使用了 `-S`。连接调试器后执行 `continue`。

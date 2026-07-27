# WaterOS RISC-V 卡死诊断：stall-debug 与 GDB/LLDB

本文记录一次真实的 WaterOS SMP 卡死排查方法。目标不是讲完 GDB，而是让第一次使用
GDB 的人能够完成下面这条链路：

1. 用带符号的内核启动 QEMU；
2. 卡住时暂停全部 hart；
3. 查看每个 hart 的 `pc`、`ra`、`sp`；
4. 把地址还原成 Rust 函数；
5. 反汇编锁的自旋循环；
6. 根据多个 hart 的位置判断死锁或锁顺序反转。

## 1. 三种调试开关

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

## 3. macOS：使用 LLDB 连接 QEMU

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

如果已经安装支持 RISC-V 的 GDB：

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

## 10. 常见问题

### 端口 1234 已被占用

```bash
lsof -nP -iTCP:1234 -sTCP:LISTEN
```

换端口启动：

```bash
WOS_QEMU_GDB_PORT=1235 make rv_final_gdb_run
```

连接时也改成 1235。

### 地址全部解析成错误函数

几乎总是 QEMU 内核与 LLDB/GDB 打开的 ELF 不一致。停止 QEMU，重新构建并确认
`WOS_KERNEL` 指向同一个文件。

### 连接后系统不再输出

调试器连接或命中断点时 guest 是暂停的。执行 `continue` 才会恢复。

### `rv_final_gdb_wait` 启动后没有任何内核输出

这是预期行为：QEMU 使用了 `-S`。连接调试器后执行 `continue`。

# Ring-buffer Pipe 实现手册

[IPC 总览](../../../README.md) · [Pipe API](../../pipe-api/api-v0/README.md) · [VFS FD Session](../../../../wateros-vfs/vfs-impl/impl-fd-session/README.md)

本 crate 实现匿名 pipe 和文件系统 FIFO 共用的可增长存储 ring buffer、端点引用、阻塞
等待、packet mode、poll 以及“用户复制完成后才消费”的 read lease。它不管理 fd table，
也不把 `PipeError` 映射为 errno/SIGPIPE。

## 1. 文件和对象图

| 文件 | 职责 |
|---|---|
| `kernel_pipe.rs` | `PipeState`、数据/segment、lease、waitqueue、read/write/poll |
| `endpoint.rs` | `PipeEndpoint`、OFD flags、clone/close、匿名 pair 和 `NamedPipe` open |
| `lib.rs` | 再导出和端到端实现自检 |

```text
fd/OFD wrapper
  -> PipeEndpoint { Arc<Pipe>, direction, shared flags, per-wrapper closed }
     -> Pipe
        ├─ Mutex<PipeState>
        ├─ read_wait: WaitQueue
        └─ write_wait: WaitQueue
```

所有缓冲、segment、reservation 和端点计数都由同一 `state` 自旋锁保护。任何 scheduler
等待和 `wake_all` 都在释放 state 锁后执行。

## 2. `PipeState` 精确数据结构

- `buf: VecDeque<u8>`：从队头读、队尾写；存储随 payload 增长，不按逻辑 capacity 预分配；
- `capacity: usize`：逻辑上限，始终非零；
- `segments: VecDeque<PipeSegment>`：描述 buf 中连续 stream/packet 段；
- `read_reservation: Option<ReadReservation>`：全 pipe 同时最多一个活动 lease；
- `next_reservation_id: u64`：wrapping 后跳过 0，防止旧 lease 提交新 reservation；
- `read_refs/write_refs`：实际打开的端点 wrapper 数；
- `read_open/write_open`：最后一个该方向端点是否仍存在。

核心不变量：

```text
buf.len() <= capacity
sum(segments.len) == buf.len()
read_open == (read_refs != 0)   // 端点模式稳定后
write_open == (write_refs != 0)
read_reservation 至多一个，且 exposed <= 首 segment.len
```

普通 stream 写会与末尾普通 segment 合并；packet 每次写形成独立 segment。buf 和 segments
必须同步消费，修改任一侧都要验证长度总和。

匿名 `Pipe::new()` 的底层初始 open 位为 true，即使直接使用 `KernelPipe` 时 refs 为 0；
`PipeEndpoint::pair` 随后各 acquire 一次。FIFO 的 `new_named()` 初始两方向均关闭，open
完全由当前文件描述维护。

## 3. 容量和内存语义

默认容量来自 base config。`set_capacity(n)`：n=0 拒绝；只要 buf 非空就返回
`InvalidCapacity`；空时直接改变逻辑上限并清空空的 segments。实现没有 Linux 的权限、
page 对齐、最小/最大 capacity 限制，这些应由 syscall/fcntl 层补齐。

把容量改到 1 MiB 不会立刻分配 1 MiB。写入时 `buf.try_reserve_exact(count)` 失败返回
`NoMemory`。但当前 `segments.push_back` 没有使用 fallible reserve：尤其 packet storm 下
segment 元数据扩容仍可能触发全局 allocator panic。这是现有 OOM 缺口；修复时应在写
buf 前为新 segment 预留，任一 reserve 失败都不得改变 buf/segments。

## 4. stream read/write

非阻塞读：

```text
try_read(out)
  -> out 为空：0
  -> 有 active reservation：WouldBlock
  -> buf 空且 writer open：WouldBlock
  -> buf 空且 writer closed：0 (EOF)
  -> 从首 segment/队头复制并消费 -> 锁外 wake writers
```

阻塞 `read` 在 WouldBlock 后进入 `read_wait.wait_current_while`，条件是“有 reservation，或
空且 writer open”；被信号打断返回 `Interrupted`，唤醒后必须重新循环检查条件。

非阻塞 stream write 在无 reader 时优先 `BrokenPipe`，满时 `WouldBlock`，否则写尽可能多
的空闲容量并唤醒 readers。阻塞 write 循环直到输入完成；若已部分写入后被打断或断管，
返回已写字节数，只有零进度才返回错误。syscall 层负责把零进度 BrokenPipe 变成 EPIPE
并按 Linux 规则生成 SIGPIPE。

当前 stream 写没有实现“`<= PIPE_BUF` 多 writer 原子写”的预留规则：有部分空间时可能
部分写入。若 LTP 依赖 POSIX 原子性，应在锁内对小写检查 `free_len >= input.len`，不足时
整体等待/`EAGAIN`，并为多 writer 竞争增加测试。

## 5. packet mode (`O_DIRECT` pipe)

`PIPE_BUF=4096`。一次 `try_write_mode(input,true)` 的 packet 长度是
`min(input.len, 4096, capacity)`，空闲不足整个 packet 时 `WouldBlock`，不会写半包。阻塞
写对大输入循环，因此拆成多个最多 4096 字节的 packet。

读取只暴露首 packet 的 `min(packet_len, user_len)`。成功完成时，无论 user buffer 是否
较短，都会消费整个 packet，未暴露尾部被丢弃；绝不与下一 packet 合并。

端点的 `direct: Arc<AtomicBool>` 是 OFD status：同方向 clone 共享，匿名 pair 的读/写
方向各自独立。切换某一端 direct 不会自动切另一端，segment 自身记录写入时的模式，
所以同一 pipe 可按历史顺序混合 stream/packet segment。

## 6. read lease：用户复制事务边界

syscall 不能在持 pipe 锁时 `copy_to_user`，也不能先消费后因 EFAULT 丢数据：

```text
acquire_read_lease(max_len)
  -> 预分配 staging，大小预算 min(max_len, capacity)
  -> 等待无 active reservation 且有数据，或 writer 已关闭
  -> 锁内复制队头到 staging，登记 {id, exposed, consume_on_success, packet}
  -> 解锁，把 Box<dyn PipeReadLease> 交给 syscall
copy_to_user(lease.bytes())
  -> lease.finish(copied, complete)
```

活动 lease 不从 buf 移走数据，也不释放写容量，并串行阻挡其它 reader。EOF lease 没有 ID、
bytes 为空，finish 恒返回 `Bytes(0)`。

stream finish：完整或部分成功只消费 copied 前缀；`copied=0 && !complete` 返回 Fault 且
不消费。packet finish：complete=true 消费整包并返回 copied；失败且 copied=0 保留整包；
失败但 copied>0 返回 Fault，同时丢弃整包，防止把一个 packet 拆成后续可读尾段。

Drop/取消只清 reservation、不消费数据，并唤醒 reader/writer。最后读端关闭会主动取消
reservation；旧 lease 再 finish 因 ID 不存在返回 Closed。调用方必须保证 copied 不超过
`bytes().len()`，否则返回 Closed。

零长度 read 应由 syscall 在 acquire 前直接返回 0。否则 packet lease 的
`finish(0,true)` 会按“成功截断”消费整个 packet，这是接口组合上的危险边界。

## 7. 端点 clone、flags 和 close

`PipeEndpoint` 中 `closed: Cell<bool>` 属于每个 Rust wrapper；`nonblocking/direct` 分别是
同方向 clone 共享的 `Arc<AtomicBool>`。clone 一个未关闭端点会增加底层方向 refs；clone
已关闭 wrapper 得到的仍是 closed wrapper，不增加 refs。

显式 `close()` 和 Drop 都进入 `release_once`，Cell 保证至多减一次。最后 reader 离开：
`read_open=false`、取消 reservation、锁外唤醒两队列；writer 随后看到 BrokenPipe。最后
writer 离开：`write_open=false`、唤醒 readers；剩余数据读完后得到 EOF。

错误方向操作和 Closed 分开：已关闭端点先返回 Closed；在 write end 上 read 或 read end
上 write 当前返回 BrokenPipe，而不是 BadFd，errno 精化在 fd/syscall 层处理。

## 8. FIFO open 状态机

`NamedPipe` 自身只持 `Arc<Pipe>`，没有隐藏 sentinel reader/writer：

- nonblocking read 立即创建读端；无 writer 时随后 read 得 EOF；
- blocking read 先创建读端，再等 `write_refs != 0`；
- nonblocking write 在无 reader 时立即 BrokenPipe（syscall 应映射 FIFO open 的 ENXIO）；
- blocking write 创建写端后等 `read_refs != 0`；
- 等待被中断时局部 endpoint Drop，自动撤销已增加的引用。

acquire 从 0 重新打开某方向时会唤醒对端 open/I/O 等待者。

## 9. poll 语义

读端：无 active lease 且“有数据或 writer closed”给 POLLIN；writer closed 额外给 POLLHUP。
写端：reader open 且未满给 POLLOUT；reader closed 给 POLLHUP|POLLERR。HUP/ERR 不受调用
方 requested events 过滤，POLLIN/POLLOUT 才过滤。

poll wait 使用同一 read/write waitqueue和 `still_waiting` 闭包，在 scheduler 注册等待时
原子复查条件以避免 lost wake。方向不匹配的 requested bit 不等待。Interrupted 映射为
PipeError::Interrupted；TimedOut/Woken 都返回 Ok，poll engine 再重新采样 revents。

packet writer 的 poll POLLOUT 当前只检查“未满”，不保证有空间容纳下一个完整 packet。
因此 poll 后 write 仍可 EAGAIN，这是允许的就绪竞态，但高频程序应正确重试。

## 10. 锁和唤醒表

| 状态改变 | 解锁后唤醒 |
|---|---|
| 写入字节 | `read_wait.wake_all()` |
| 消费/提交数据 | freed 时 `write_wait`，并唤醒被 lease 串行的 readers |
| lease Drop | read + write wait |
| reader refs 0→1 | write wait（FIFO open） |
| writer refs 0→1 | read wait（FIFO open） |
| 最后 reader close | read + write wait |
| 最后 writer close | read wait |

禁止在持 `PipeState` 锁时进入 waitqueue/scheduler。条件闭包可以短暂重新取 state 锁，但
不得调用 VFS、user copy 或日志。

## 11. 扩展 `splice` 实例

实现 pipe→pipe splice 时不要直接同时持两把 state 锁然后猜顺序。建议：

1. 定义全局稳定锁序（例如按 Arc 内对象地址），同 pipe 特判；
2. 源端建立内部 reservation，目标端预留容量/segment 元数据；
3. packet 必须保持边界与截断语义，stream 小写原子性不能退化；
4. 目标提交成功后才能消费源；失败/中断 Drop 两边 reservation；
5. user page splice 若引入 page 引用，重新定义 capacity 单位和 Drop 所有权；
6. 所有 wake 在两把锁都释放后执行；
7. 测试反向双 pipe 并发 splice，证明没有 ABBA。

## 12. 故障与回归

重点诊断：buf 增长而 segments 不一致、active lease 永久存在、refs 归零遗漏、持 state 锁
等待、packet 元数据 OOM，以及 syscall 把部分成功误报为 EFAULT/EINTR。

自回归至少覆盖：

- 空/满、1 字节环绕、容量调整及 payload 比 1 MiB 逻辑容量小；
- stream 多 writer 的 PIPE_BUF 原子性（当前缺口需先补实现）；
- packet 4096 分片、短读丢尾、混合 stream/packet；
- lease 完成、零/部分 EFAULT、Drop、最后 reader close 和旧 ID；
- clone 后 flags 共享、pair 两方向 flags 独立、显式 close+Drop 幂等；
- FIFO blocking/nonblocking open、中断和最后 writer EOF；
- poll 数据/HUP/ERR、timeout、信号和注册竞态；
- fallible buf 与 segment 元数据 OOM；
- 多 reader/writer SMP 压力及关闭竞态。

```sh
python3 scripts/maintenance/check_offline_docs.py
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```

当前不能在宿主机直接运行本 crate 的 `cargo test --manifest-path ...`：waitqueue 间接依赖
`wateros-task`，该独立入口没有选择 `platform-arch` 实现，会在测试代码执行前因
`ArchTimeImpl/ArchInterruptImpl/ArchPagingImpl` 未定义而编译失败。这不是用例失败。现有
行为测试位于 `lib.rs::test()`，通过顶层 IPC/kernel `self_test` feature 在目标架构执行。
若要恢复 host unit test，应给 waitqueue 注入 mock scheduler/time backend，而不是在 x86
测试中强行选择包含目标架构寄存器/汇编的 RISC-V 或 LoongArch 实现。

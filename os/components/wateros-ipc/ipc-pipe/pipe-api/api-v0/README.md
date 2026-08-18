# pipe API v0 离线开发手册

本文说明匿名 pipe 的稳定接口。整体设计见 [ipc-pipe](../../readme.md)，当前实现见
[impl-ringbuf](../../pipe-impl/impl-ringbuf/README.md)，等待规则见
[ipc-waitqueue](../../../ipc-waitqueue/readme.md)。

## 1. 边界与核心对象

API crate 定义错误、方向、内核 pipe trait、fd 端点 trait 和锁外读取 lease。它不管理 fd
编号、不复制用户内存、不产生 `SIGPIPE`，也不拥有调度器。

```text
sys_pipe2/read/write/poll/fcntl
  -> VFS fd-session：fd、flags、user-copy、errno、SIGPIPE
  -> PipeEndpointOps：端点方向与 close 生命周期
  -> KernelPipe：缓冲、EOF、阻塞和 readiness
  -> waitqueue/task scheduler
```

## 2. 稳定类型和错误

`PipeEndpointKind` 只有 `Read` 与 `Write`。`read_checked/write_checked` 会先检查方向；当前
错误方向被归为 `BrokenPipe`，syscall 层若要返回更精确的 Linux errno，必须在 ABI 层处理。

| `PipeError` | 常见 errno | 触发条件 |
| --- | --- | --- |
| `WouldBlock` | `EAGAIN` | 非阻塞且暂时不可读/写 |
| `Interrupted` | `EINTR` | 阻塞等待被信号打断且无部分进度 |
| `BrokenPipe` | `EPIPE` | 写时已经没有读端 |
| `Closed` | `EBADF` | 当前端点已显式关闭 |
| `InvalidCapacity` | `EINVAL` | 容量为零或非空时调整容量 |
| `NoMemory` | `ENOMEM` | staging/ring payload 分配失败 |

`DEFAULT_PIPE_CAPACITY` 来自 base-config。配置容量是上限，不应在创建每个 pipe 时立即预留
同等大小的内核堆；当前实现按实际 payload 增长。

## 3. `KernelPipe` 契约

- `with_capacity/new` 创建缓冲；容量必须非零；
- `capacity/len` 返回限制和当前字节数；
- `set_capacity` 当前只允许空 pipe 调整；
- `try_read/try_write` 从不主动睡眠；
- `read/write` 在条件仍不满足时阻塞；
- poll 方法返回 Linux 原始 readiness 位。

读语义：有数据返回字节；空且仍有写端时 `WouldBlock` 或等待；空且最后一个写端关闭时
返回 `Ok(0)`，即 EOF。写语义：有空间时写入；满且仍有读端时 `WouldBlock` 或等待；最后
一个读端关闭后返回 `BrokenPipe`。阻塞写已有部分进度再遇信号或对端关闭时允许返回已写
字节数。

`write(&[])` 和 `read(&mut [])` 返回 0，不应因为对端状态而睡眠。

## 4. 端点和引用生命周期

`PipeEndpointOps::pair(nonblocking)` 创建一读一写两个端点。每个 wrapper 有一次性的 close
位；底层按方向维护引用计数：

```text
pair/open       -> 对应 read_refs/write_refs +1
dup/fork/Clone  -> 对应方向引用 +1
close           -> 当前 wrapper 至多释放一次
Drop            -> 尚未 close 时释放一次
引用归零         -> read_open/write_open=false，锁外 wake 对端
```

`Clone` 不是单纯复制 `Arc`，必须增加底层端点引用。`close` 与 `Drop` 必须幂等，否则引用
提前归零会制造假 EOF/EPIPE，重复不减则会让对端永久等不到 EOF。

当前实现让同一 open-file-description 的 clone 共享 `O_NONBLOCK/O_DIRECT` 原子标志，而每个
wrapper 独立保存 closed 位。这与 fd duplicate 的状态共享关系有关，新增 flag 时要先判断它
属于 descriptor 还是 open file description。

## 5. 条件等待和锁顺序

`PipeState` 锁只保护 payload、segment、reservation、容量和端点引用。正确顺序是：

```text
锁内检查/修改 PipeState
  -> 释放 PipeState 锁
  -> wait_current_while(短暂重锁并复查条件) 或 wake_all
```

读者等待“当前为空、写端仍打开，或另一个 read reservation 尚未结束”；写者等待“空间
不足且读端仍打开”。闭包由 scheduler 原子复查，封闭检查到入队的 lost-wake 窗口。任何
唤醒都只是提示，醒来必须循环复查。

不能持 `PipeState` 锁进入 wait/wake；不能在 scheduler 条件闭包里做 user-copy。被唤醒
任务的 CPU 和 IPI 全由 task scheduler 决定。

## 6. 为什么需要 `PipeReadLease`

直接把用户指针交给 pipe 实现会在对象锁内发生缺页。lease 把操作拆为三段：

1. `acquire_read_lease(max_len)` 在锁内把当前可读记录复制到内核 staging，并建立唯一
   reservation；
2. fd/syscall 层在无 pipe 锁时把 `lease.bytes()` 复制给用户；
3. 无论成功或 fault，都消费 lease，调用 `finish(copied, complete)`；若直接 Drop，则取消
   reservation，数据保持未消费。

同一时刻只有一个 read reservation。stream mode 提交已成功复制的前缀；若 0 字节即 fault，
保留数据。packet mode 完整复制时消费整个 packet；发生部分 user-copy fault 时仍按记录语义
消费整个 packet并返回 `PipeReadFinish::Fault`，0 字节 fault 则保留。EOF lease 的 bytes 为空，
finish 返回 `Bytes(0)`。

每个取得的 lease 必须 finish 或 Drop。遗失 reservation 会让读/poll 永久 `WouldBlock`。

## 7. stream 与 packet mode

普通写形成 stream segment，相邻 stream 写可合并，读取按请求长度消费。当前实现没有完整
保证多 writer 下小于 `PIPE_BUF` 的 stream 写绝不交错，不能据此声称完整 Linux 原子写语义。

`O_DIRECT` pipe 使用 packet mode：

- 单个 packet 最大 `PIPE_BUF=4096`，也不超过 pipe 容量；
- 空间不足以容纳整个本次 packet 时等待或 `WouldBlock`；
- 每次读取最多返回一个 packet，不与下一 packet 合并；
- 用户缓冲较小时，当前 packet 未暴露的尾部会被丢弃。

`O_DIRECT` 在这里不是文件 direct-I/O。新增 splice/vmsplice 时不能绕过 segment 与
reservation 元数据。

## 8. poll 契约

读端：有数据且无 reservation 时 `POLLIN`；写端关闭时 EOF 也可读并带 `POLLHUP`。写端：
读端存在且有空间时 `POLLOUT`；读端关闭时 `POLLHUP|POLLERR`。

`poll_revents(events)` 只做无阻塞快照并与请求 mask 相交。`poll_wait_for_ticks` 只在本 pipe
队列等待；多 fd poll 的主循环必须维护共享的 `still_waiting` 条件，任一 fd 就绪后让其它
等待结束并重新扫描全部 fd。信号中断映射为 `Interrupted`。

## 9. 新增 syscall/功能实例：`F_SETPIPE_SZ`

1. fd-session 验证 fd 是 pipe，并取得 endpoint；
2. 做 Linux 权限、上下限和页对齐策略；
3. 调用实现的 `set_pipe_capacity`，不要直接访问 `PipeState`；
4. 明确当前实现只允许空 pipe，非空返回的领域错误映射 `EINVAL`；
5. 成功后唤醒可能因旧容量而阻塞的 writer/poll waiter；若实现内部已唤醒，避免重复协议；
6. 容量只是限制，存储仍按 payload 延迟分配；
7. 测试 0、缩小、扩大、非空、并发 writer、内存分配失败和 clone 可见性。

若要增强为 Linux 完整行为，应先设计非空 ring 数据迁移的原子提交与失败回滚，再放宽 API，
不能先改容量字段后在锁外搬数据。

## 10. 故障与验证

- 永久阻塞：查端点引用是否泄漏、等待条件是否与 wake 条件一致；
- 提前 EOF/EPIPE：查 clone/close/drop 是否恰好加减一次；
- 堆增长：查大容量 pipe 是否预分配、lease staging 是否被长期持有；
- 读数据重复/丢失：查 reservation 的 finish/fault/drop 分支；
- poll 空转：查 EOF/HUP 与 reservation readiness；
- packet 串包：查 segment 队列是否与 payload 同步消费。

回归至少覆盖 stream/packet 回卷、部分读写、EOF、EPIPE、nonblocking、signal、poll、dup/fork、
显式 close 后 Drop、user-copy fault 和并发 reader/writer；再运行双架构 `make check`。


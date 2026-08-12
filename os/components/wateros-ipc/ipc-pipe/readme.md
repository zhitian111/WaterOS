# ipc-pipe

[项目首页](../../../../README.md) · [内核工程](../../../README.md) · [系统架构](../../../../README.md#系统架构)

`ipc-pipe` 提供内核内部匿名 pipe：固定容量 ring buffer、读写端引用计数、阻塞/非阻塞
I/O，以及 `poll` 等待。它不管理 fd 表；VFS fd-session 层把 `PipeEndpoint` 包装为文件句柄。

## 分层

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 聚合 | `src/lib.rs` | 选择 ring-buffer 实现、重导出 fd 可持有的 `PipeEndpoint`。 |
| API | `pipe-api/api-v0/` | `KernelPipe`、`PipeEndpointOps`、端点方向与错误。 |
| 实现 | `pipe-impl/impl-ringbuf/` | `PipeState`、ring buffer、端点引用和两个等待队列。 |
| 调用方 | `vfs-impl/impl-fd-session` | fd 生命周期、`pipe2`/`fcntl`/`poll` 的 Linux ABI。 |

## 数据和流程

`PipeState` 由一个自旋锁保护：环形字节缓冲、head、len、读/写端开放状态及对应引用计数。
`Pipe` 额外持有两个 `ipc-waitqueue::WaitQueue`：

- `read_wait`：空 pipe 且仍有写端时，读者与读侧 poll 在此等待；写入或关闭最后一个写端时唤醒。
- `write_wait`：满 pipe 且仍有读端时，写者与写侧 poll 在此等待；读取或关闭最后一个读端时唤醒。

```text
read:  try_read -> 空且 writer 存在 -> read_wait 条件等待 -> 重试
write: try_write -> 满且 reader 存在 -> write_wait 条件等待 -> 重试
close: 最后一个端点引用释放 -> 改变 open 状态 -> 唤醒对侧等待者
```

读取空且写端关闭返回 `Ok(0)`（EOF）；写入读端已关闭返回 `BrokenPipe`；非阻塞 I/O 在
暂时无法进行时返回 `WouldBlock`。

## 并发与 SMP

- `PipeState` 锁不跨越 `WaitQueue::wait_*` 或 `wake_*` 调用。先更新状态、释放锁，再唤醒。
- 条件等待在 task scheduler 的临界区重新检查 pipe 状态，避免“状态改变后仍睡眠”的 lost wake。
- 等待队列唤醒后的 CPU 选择、远端入队和定向 IPI 都由 `wateros-task` 处理；pipe 不固定目标 CPU。
- `PipeEndpoint::closed` 保证显式 `close` 与 `Drop` 至多释放一次端点引用。
- 已关闭端点不再允许 read、write、poll 或调整容量；fd-session 应将其映射为 `EBADF`。

## 当前限制

- 缓冲区仅在 pipe 为空时允许调整；没有 Linux 的 `PIPE_BUF` 原子写入保证。
- `O_DIRECT` 目前只是端点标记，尚未实现 packet-mode 截断语义。
- poll 使用原始 Linux 位值，但完整多 fd poll 的重扫策略由 VFS/syscall 层负责。

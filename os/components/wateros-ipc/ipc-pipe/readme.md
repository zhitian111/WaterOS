# ipc-pipe

[项目首页](../../../../README.md) · [内核工程](../../../README.md) · [wateros-ipc](../readme.md)

`ipc-pipe` 提供 WaterOS 内核中的匿名 pipe：固定容量 ring buffer、读写端生命周期、阻塞和
非阻塞 I/O，以及 poll readiness。它不管理 fd 表，VFS fd-session 层负责把 PipeEndpoint
包装成文件句柄。

## 模块分层

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 聚合层 | `src/lib.rs` | 选择 ring-buffer 实现并导出 PipeEndpoint。 |
| Pipe API | `pipe-api/api-v0/` | 定义 KernelPipe、端点 trait、方向、lease 和错误。 |
| Pipe 实现 | `pipe-impl/impl-ringbuf/` | 实现 PipeState、ring buffer、端点引用和等待队列。 |
| 等待适配 | `ipc-waitqueue/` | 将 pipe 条件等待和唤醒委托给 task scheduler。 |
| VFS/fd 层 | `wateros-vfs/vfs-impl/impl-fd-session/` | fd 生命周期、pipe2、fcntl、read/write 和 poll ABI。 |

实现文件按职责拆分如下：

| 文件 | 内容 |
| --- | --- |
| `pipe-api/api-v0/src/kernel_pipe.rs` | KernelPipe 创建契约。 |
| `pipe-api/api-v0/src/endpoint.rs` | 端点方向、操作 trait 和 read lease。 |
| `pipe-api/api-v0/src/error.rs` | PipeError 与默认容量。 |
| `pipe-impl/impl-ringbuf/src/kernel_pipe.rs` | 共享 Pipe、PipeState、ring buffer 和等待队列。 |
| `pipe-impl/impl-ringbuf/src/endpoint.rs` | NamedPipe、PipeEndpoint 和端点生命周期。 |

## 实现说明

- 一个 pipe 由共享 PipeState、`read_wait` 和 `write_wait` 组成，多个端点引用同一底层对象。
- PipeState 锁只保护缓冲、head、len 和读写端计数，不得跨越 WaitQueue 等待、唤醒或调度。
- 空 pipe 且仍有写端时，阻塞读等待；满 pipe 且仍有读端时，阻塞写等待。
- 空 pipe 且所有写端关闭时读返回 EOF；所有读端关闭时写返回 BrokenPipe，由上层产生 SIGPIPE。
- 非阻塞端点在暂时无法进行 I/O 时返回 WouldBlock。
- 条件等待在 scheduler 临界区再次检查 PipeState，避免状态改变后任务仍然睡眠。
- 被唤醒任务的 CPU 选择、远端入队和 IPI 由 task scheduler 完成，pipe 不绑定 CPU。
- 当前没有 Linux `PIPE_BUF` 原子写入保证，`O_DIRECT` 仅记录标志，尚未实现 packet mode。

## 调用链路

创建流程：

```text
pipe2
  -> fd-session 请求创建一对 PipeEndpoint
  -> Pipe 分配共享 ring buffer、read_wait 和 write_wait
  -> 创建一个读端和一个写端
  -> fd-session 分配两个 fd 并保存端点
```

读取流程：

```text
read(fd)
  -> PipeEndpoint::try_read
  -> 有数据：从 ring buffer 读取并更新 head/len
  -> 释放 PipeState 锁后唤醒 write_wait
  -> 空且无写端：返回 EOF
  -> 空且 nonblocking：返回 WouldBlock
  -> 空且有写端：read_wait.wait_current_while(仍为空且有写端)，唤醒后重试
```

写入流程：

```text
write(fd)
  -> PipeEndpoint::try_write
  -> 有空间：写入 ring buffer 并更新 len
  -> 释放 PipeState 锁后唤醒 read_wait
  -> 无读端：返回 BrokenPipe
  -> 满且 nonblocking：返回 WouldBlock
  -> 满且有读端：write_wait.wait_current_while(仍满且有读端)，唤醒后重试
```

关闭流程：

```text
close / Drop
  -> 幂等关闭当前 endpoint
  -> PipeState 锁内减少相应端点引用
  -> 最后一个读端关闭：锁外唤醒 write_wait
  -> 最后一个写端关闭：锁外唤醒 read_wait
```

## PipeState实现功能

`PipeState` 定义在 `pipe-impl/impl-ringbuf/src/kernel_pipe.rs`。

- 保存固定容量字节数组、head 和当前有效长度。
- 根据 head/len 计算可连续读取和可连续写入区域，并处理环形回卷。
- 保存读端与写端引用计数及 open 状态。
- 计算当前 readable、writable、EOF 和 broken-pipe 条件。
- 仅在 pipe 为空时允许调整容量，避免搬移尚未消费的数据。

PipeState 只描述内存状态，不直接阻塞任务。调用方在锁内得到“需要等待/需要唤醒”的结论后，
必须释放状态锁再操作 WaitQueue。

## PipeEndpoint实现功能

`PipeEndpoint` 定义在 `pipe-impl/impl-ringbuf/src/endpoint.rs`。

- 每个 endpoint 记录 Read/Write 方向以及 nonblocking、direct 等端点标志。
- 对错误方向的 read/write 拒绝操作，关闭后的端点也不再允许 I/O、poll 或容量调整。
- Clone 增加相应方向的引用，显式 close 或 Drop 减少引用。
- endpoint 内部 `closed` 状态保证显式 close 与 Drop 至多释放一次底层引用。
- `NamedPipe` 将共享 Pipe 与可传递端点组合起来，供聚合层和 fd-session 使用。
- PipeReadLease 支持上层分阶段消费读取结果，同时保持读取完成语义可控。

## Pipe等待与Poll实现功能

- `read_wait` 保存等待“有数据或写端全部关闭”的读者和读侧 poll waiter。
- `write_wait` 保存等待“有空间或读端全部关闭”的写者和写侧 poll waiter。
- 阻塞路径优先使用条件等待，不采用“先检查、再无条件睡眠”的失唤醒写法。
- poll 只计算当前端点 readiness；多 fd 注册、等待后重扫和 Linux poll 位转换由 VFS/syscall
  层协调。
- 端点关闭和数据读写会在 PipeState 解锁后唤醒可能受影响的对侧 waiter。

## Pipe聚合层实现功能

`ipc-pipe/src/lib.rs` 负责导出 `api-v0` 和 `impl-ringbuf`：

- 对外提供 KernelPipe、PipeEndpointOps、端点方向、lease、错误和默认容量。
- 导出当前实现的 NamedPipe 与 PipeEndpoint。
- 调用方应通过 `ipc::pipe` 或 VFS fd-session 使用端点，不直接访问 PipeState 或实现锁。

排查 pipe 卡顿时，应同时检查缓冲 len、读写端引用和对应 WaitQueue：常见问题不是 ring buffer
本身，而是最后一个端点未释放、条件等待闭包与状态不一致，或唤醒发生在仍持有对象锁时。

# RIO-05：pipe 与 Unix socket 读取提交

## 任务目标

让 pipe、Unix stream 和 Unix datagram 在用户拷贝失败时不丢数据，并在并发 reader、
writer、dup/fork 和 close 下保持顺序、容量与唤醒正确。

## 前置条件

- RIO-02 user-copy 进度已合入。
- RIO-03 OFD 共享状态已合入。
- RIO-04 `VfsReadLease` 契约已合入。

## 执行前必读

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`
- `docs/exports/features/wateros-ipc.md`
- `docs/exports/public-api/wateros-ipc.md`
- `docs/exports/impl-guide/wateros-ipc.md`
- `docs/exports/features/wateros-vfs.md`
- `docs/exports/public-api/wateros-vfs.md`
- `docs/exports/impl-guide/wateros-vfs.md`
- `docs/tasks/read-family/README.md`
- `docs/tasks/read-family/04-vfs-read-lease-and-files.md`

## 已知信息与代码证据

pipe `read_into()` 会立即修改 head/len：

```rust
self.head = (self.head + count) % capacity;
self.len -= count;
```

Unix datagram 读取同样先 `pop_front()`，随后 syscall 才执行 user-copy：

```rust
if let Some((packet, _)) = pop_dgram_packet(&mut inner) {
    buf[..n].copy_from_slice(&packet[..n]);
    return Ok(n);
}
```

因此 `copy_to_user` 的 `EFAULT` 会永久丢失 pipe 字节或整个 datagram。

## 涉及文件

- `os/components/wateros-ipc/ipc-pipe/pipe-api/api-v0/src/kernel_pipe.rs`
- `os/components/wateros-ipc/ipc-pipe/pipe-api/api-v0/src/endpoint.rs`
- `os/components/wateros-ipc/ipc-pipe/pipe-impl/impl-ringbuf/src/kernel_pipe.rs`
- `os/components/wateros-ipc/ipc-pipe/pipe-impl/impl-ringbuf/src/endpoint.rs`
- `os/components/wateros-vfs/vfs-impl/impl-fd-session/src/handles.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/unix_sock.rs`
- `os/components/wateros-vfs/vfs-api/api-v0/src/handle.rs`

## 任务内容

让 pipe、Unix stream 和 Unix datagram 都接入 RIO-04 的读取租约。必须将
“观察数据”和“确认消费”拆开，并保持每种对象原有的阻塞、唤醒和消息边界语义。

### Pipe reservation 不变量

在 `PipeState` 中增加至多一个 active read reservation，或实现等价 generation/token：

```rust
struct ReservedPipeRead {
    id: u64,
    data: Vec<u8>,
}

struct PipeState {
    // existing ring state...
    read_reservation: Option<ReservedPipeRead>,
}
```

必须满足：

1. begin 在短锁内从队首保留字节，但 reservation 未结束前其它 reader 不得越过它。
2. 锁外进行 user-copy。
3. stream partial fault 时只消费已复制前缀，未提交后缀回到逻辑队首。
4. 被保留但未提交的字节继续计入 pipe 容量；writer 不能利用临时腾出的空间导致回滚
   后超过 capacity。
5. commit/cancel 后分别唤醒 reader/writer。
6. lease Drop、task exit、fd close 和 signal interrupt 都会取消 reservation。
7. 不在持 `PipeState` spin lock 时进入 user-copy 或 waitqueue sleep。

等待 active reservation 的 reader 使用现有 WaitQueue 协议，不能 busy-yield。close
写端时，已有保留字节仍须先被读取；close 读端时取消 reservation 并唤醒 writer。

## Unix stream

`UnixStreamPairEnd` 基于 pipe endpoint，优先复用 pipe lease，不复制另一套 ring buffer
事务。确认 `socketpair`、dup 和 fork 都共享同一 endpoint/OFD 状态。

## Unix datagram

datagram 需要独立的记录型 reservation：

- begin 保留队首 packet 和 sender metadata；
- copy 全部成功后消费整包，即使用户 buffer 比 packet 小也按 Linux truncation 规则
  丢弃未返回的报文尾部；
- 首字节 `EFAULT` 时整包保持队首；
- 跨页部分 fault 的用户可见返回和是否消费整包必须先在宿主 Linux 做差分测试，再固化
  到 lease `finish` 策略；
- `recvfrom_unix()` 与通过 `read()` 进入的路径必须共用同一队列协议。

不能只修 `UnixSocketHandle::read()` 而遗漏 `recvfrom_unix()`。

## 已知并发风险

- active reservation 与另一个 reader；
- reservation 期间 writer 填满剩余容量；
- reservation 期间 close 任一端；
- signal 中断等待 reservation 的任务；
- lease owner task 被 `exit_group` 终止；
- O_DIRECT packet-mode pipe：一次 read 只处理一条 packet，规则不同于 byte stream。

O_DIRECT pipe 必须保留 packet 边界；不能把未提交后缀与下一 packet 合并。

## 如何验收

组件测试至少包括：

- `"abcdef"` 保留 6 字节，copy 3 后 fault，下一 read 得到 `"def"`；
- copy 0 后 fault，下一 read 得到完整原数据；
- reservation 期间 writer 不突破 capacity；
- 第二 reader 不越过 reservation；
- lease Drop 自动回滚；
- writer close 后先读完保留数据再 EOF；
- read end close 唤醒阻塞 writer；
- O_NONBLOCK 空 pipe 仍为 `EAGAIN`；
- O_DIRECT packet 边界不变；
- Unix datagram EFAULT 后 packet/sender 不丢失。

运行：

```bash
cd os
make rv_check
make la_check
```

RIO-10 中再运行 LTP pipe、socketpair、fork/dup 并发及跨页 EFAULT 测例。

## 搜索范围、并行与交付

用 `rg "read_into|pop_front|PipeEndpoint|UnixSocketHandle|recvfrom_unix"` 审核
`wateros-ipc/ipc-pipe`、fd-session handles 和 `unix_sock.rs` 的所有消费入口。

本任务可与 RIO-06、RIO-07、RIO-08 并行，但只使用 RIO-04 已合入的 lease API。测试
分别放 pipe impl 和 syscall/Unix socket 相关测试；日志放 `/tmp`。完成后在索引勾选
RIO-05，记录 byte-stream、packet-mode、datagram 和 close/signal 结果。

## 禁止做法

- 不先 pop/read 再在失败时无条件 push_front；并发 reader 会破坏顺序。
- 不在 reservation 时少算 pipe 占用容量。
- 不用周期性 sleep 轮询 reservation。
- 不把 stream partial commit 规则错误套到 datagram/O_DIRECT packet。

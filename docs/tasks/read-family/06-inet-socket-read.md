# RIO-06：inet TCP/UDP 非破坏性读取

## 任务目标

使 inet TCP、UDP 和内核 loopback 队列支持读取租约，确保 `read/recv/recvfrom` 在用户
拷贝失败时不错误消费接收数据，并统一同一 socket 上的并发接收串行化。

## 前置条件

- RIO-02、RIO-03、RIO-04 已合入。

## 执行前必读

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`
- `docs/exports/features/wateros-driver.md`
- `docs/exports/public-api/wateros-driver.md`
- `docs/exports/impl-guide/wateros-driver.md`
- `docs/exports/features/wateros-syscall.md`
- `docs/exports/public-api/wateros-syscall.md`
- `docs/exports/features/wateros-vfs.md`
- `docs/exports/public-api/wateros-vfs.md`
- `docs/tasks/read-family/README.md`
- `docs/tasks/read-family/04-vfs-read-lease-and-files.md`

## 已知信息与代码证据

当前 TCP 读取直接消费 smoltcp queue：

```rust
socket.recv_slice(buf)
```

UDP loopback 直接：

```rust
if let Some(packet) = queue.pop_front() { ... }
```

项目固定使用 smoltcp `0.12.0`，该版本已经提供：

```rust
tcp::Socket::peek / peek_slice / recv
udp::Socket::peek / peek_slice / recv
```

因此不需要修改 vendored/registry smoltcp。应在 `driver-network` 的 API/实现层组合
peek、受控 consume 和 socket 级 reservation。

## 涉及文件

- `os/components/wateros-driver/driver-network/src/lib.rs`
- `os/components/wateros-driver/driver-network/src/socket_handles.rs`
- `os/components/wateros-driver/driver-network/network-api/api-v0/`
- `os/components/wateros-driver/driver-network/network-impl/impl-smoltcp/`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/socket_fd.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/io.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/net/recvfrom.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/socket_block.rs`

实际 API/impl 目录以当前 feature 导出链为准，不把 smoltcp 类型泄漏进 syscall
`api-v0`。

## 任务内容

在每个 `SocketRef` 或 socket metadata 中维护接收 reservation/generation 和等待队列。
reservation 只在短时 network lock 内建立/提交，user-copy 在锁外执行。

TCP 流程：

1. poll 后确认可读。
2. 短锁设置 receive reservation，并用 `peek_slice` 复制到内核 staging。
3. 释放 `NETWORK_STACK` 锁。
4. 执行 user-copy。
5. 短锁验证 reservation，使用 `tcp::Socket::recv` 只消费已提交前缀。
6. 清 reservation，唤醒其它 reader。

TCP 接收队列在 lease 期间仍可由网络 poll 追加数据，但其它 reader 不能消费被保留的
前缀。

UDP 流程：

- smoltcp UDP 使用 `peek/peek_slice` 取得队首 datagram，不先 `recv`；
- loopback `VecDeque` 使用与 Unix datagram 相同的 record reservation；
- 成功短 buffer 按 Linux datagram truncation 规则消费整包；
- `EFAULT` 和跨页 partial fault 规则先做 Linux 差分测试；
- packet metadata（源地址、端口）与 payload 同时提交/回滚。

## syscall/VFS 边界

当前 `read_fd()` 先用 `socket_fd::lookup(fd)` 取出 `SocketRef`，随后脱离 fd/OFD 操作
上下文。接入后优先让 `TcpStreamHandle/UdpSocketHandle` 实现统一 read lease，或让
`socket_fd` 返回 driver 层 receive lease；不能继续由 syscall 直接调用
`recv_slice()`。

`read`、`recv`、`recvfrom` 必须共用同一个 socket receive reservation，否则两个入口
仍可互相越过。listen socket、未连接 socket 和已关闭 TCP 的 errno/EOF 保持现有
Linux 对照，不在本任务用统一 `EIO` 覆盖。

## 阻塞与锁

- 等待数据和等待 reservation 使用 socket waitqueue/现有 blocking helper。
- 不持 `NETWORK_STACK` 锁调用 `task::yield_now`、sleep、user-copy 或分配大 buffer。
- nonblocking socket 无数据返回 `EAGAIN`。
- signal interrupt 在尚未复制数据时返回 `EINTR` 且不消费队列。
- lease owner 退出时 Drop/cancel 必须解除 reservation。

## 如何验收

至少覆盖：

- TCP 首字节 EFAULT 后，下一次 valid read 获得完整数据；
- TCP 跨页 partial fault 只消费并返回已复制前缀；
- 两个 dup/fork reader 不重读、不跳字节；
- UDP EFAULT 后同一 datagram 和 source metadata 仍可接收；
- UDP 成功短 buffer 的 truncation 与 Linux 相同；
- loopback 与真实 virtio-net 路径行为一致；
- nonblocking、EOF、connection reset 和 signal 行为无回归；
- CAgent 连续三轮 10/10。

执行：

```bash
cd os
make rv_check
make la_check
make kernel-rv-final
```

QEMU 运行使用 final 网络参数和独立 overlay，最终记录纳入 RIO-10。

## 搜索范围、并行与交付

用 `rg "recv_slice|peek_slice|socket_recv|socket_recvfrom|pop_front|socket_fd::lookup"`
审核 driver-network、socket handles、`read`、`recv` 和 `recvfrom` 全部入口。确认固定
smoltcp 版本后使用其公开 API，不读写 Cargo registry 源码。

本任务可与 RIO-05、RIO-07、RIO-08 并行。API 放 network `api-v0`，smoltcp 细节放
impl/聚合层，syscall 只依赖稳定导出。日志放 `/tmp`。完成后在索引勾选 RIO-06，
记录 TCP/UDP、loopback/virtio、read/recvfrom 共用 reservation 及 CAgent 结果。

## 禁止做法

- 不修改 smoltcp 源码规避问题。
- 不持全局 network stack 锁跨 user-copy。
- 不让 `read` 和 `recvfrom` 使用两套接收队列锁。
- 不把 UDP 当 TCP 字节流做部分 packet 回填。

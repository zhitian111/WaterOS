# smoltcp Network 实现手册

[Network 总览](../../README.md) · [Network API](../../network-api/api-v0/README.md)

本 crate 管理唯一 smoltcp Interface、SocketSet、网卡适配器和 WaterOS 补充元数据。所有协议栈操作经全局 `TrackedMutex<Option<NetworkStack>>`；未初始化的业务操作返回 StackUnavailable，周期 poll 可无声跳过。

## 源码地图

| 文件 | 职责 |
| --- | --- |
| `adapter.rs` | `NetworkDevice` 到 smoltcp Device/Token，RX 暂存、TX 和本机帧回灌。 |
| `stack/global.rs`、`init.rs` | 单例锁、安装、IPv4 地址和路由。 |
| `stack/state.rs` | SocketMeta、listener group、close pending、UDP loopback 和容量。 |
| `stack/socket.rs` | create/bind/connect/listen/accept/shutdown/close 与快照。 |
| `stack/tcp.rs`、`udp.rs` | 协议收发。 |
| `stack/receive.rs` | 唯一接收 reservation 和精确提交。 |
| `stack/poll.rs` | iface poll、connect deadline、listener 补槽和延迟关闭回收。 |
| `stack/sockopt.rs` | socket option 元数据和底层参数。 |

## 全局状态与锁序

`NetworkStack.metas` 必须与活动 smoltcp socket 对应；一个用户 TCP listener 由 listener group 的多个底层 socket 表示，accept 取走已连接槽并补 replacement。fd 最后关闭时 UDP 立即移除，TCP 未完成 FIN 则进入 `tcp_close_pending`，由后续 poll 回收。

通常锁序是 `SocketShared.handle` → network stack → adapter 内短暂 device lock。禁止持 stack 锁等待 task、用户拷贝或 VFS；也不能从 adapter/device 回调反向调用会再次获取 stack 锁的入口。

## 轮询与数据链路

```text
后台 poller 或 syscall retry
  -> poll_at_millis(monotonic)
  -> adapter 从驱动暂存最多 32 帧
  -> iface.poll 处理 ARP/IPv4/TCP/UDP 并发送帧
  -> poll_socket_events 更新 connect/listener/close 状态

recv/read
  -> poll stack
  -> prepare_recv: reservation + peek 到 lease Vec
  -> 锁外 copy_to_user
  -> finish_recv(copied, complete) 或 lease Drop cancel
```

smoltcp 时间必须单调，`last_poll_millis` 防止时钟失败路径倒退。当前没有网卡事件直达 socket waitqueue，阻塞 syscall 依赖后台 poller和 tick 重试；增加事件唤醒时必须避免在网卡中断或 stack 锁内直接进入 scheduler 锁。

## 容量与语义陷阱

- TCP 每 socket RX/TX 各 65535 字节；listener 槽最多 16，每槽都付出该内存成本。
- UDP 数据区 64 KiB、metadata 64；最大 IPv4 payload 65507。
- 本机 UDP 当前绕过 smoltcp，使用最多 256 包/64 KiB 的 FIFO，满时丢新包但 send 仍成功。
- SO_SNDBUF/SO_RCVBUF 当前部分仅记录查询值，并不重分配已创建缓冲。
- TX device send 错误发生在 smoltcp token consume 后，目前只能记录日志，不能倒推已完成调用失败。

## 修改与回归

新增 socket 状态时同步 `SocketMeta`、control 操作、poll 事件、快照、close 和 `/proc`。任何分配多个底层 handle 的失败路径都要移除已插入 socket/meta/buffer。测试 loopback 与真实 QEMU 网卡、TCP connect 成功/拒绝/30 秒超时、backlog 并发、accept 补槽、半关闭、UDP 报文边界、reservation EFAULT、非阻塞 poll 和最终 Arc close。

内存压力测试应计算 socket 数乘固定缓冲和 listener 槽，而不只观察包流量；大量短连接还要持续 poll，确认 `tcp_close_pending` 回落且内核 heap 恢复。


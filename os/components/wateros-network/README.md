# wateros-network

[项目首页](../../../README.md) · [内核工程](../../README.md) · [系统架构](../../../README.md#系统架构)

`wateros-network` 是 WaterOS 的网络协议栈聚合模块。它提供后端无关的 socket 语义类型
（`network-api`）、基于 smoltcp 的 IPv4 协议栈实现（`impl-smoltcp`），以及 socket 到 VFS
fd 的桥接（`vfs-handles`）。syscall 层通过 `api` 类型与 `stack` 稳定调用面使用协议栈，
不直接依赖 smoltcp。

## 模块分层

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 聚合门面 | `src/lib.rs` | 再导出 `api` 与 socket 语义类型；启用 `impl-smoltcp` 时提供 `stack` 稳定调用面；`vfs-handles` 时提供 socket→VFS fd 桥接。 |
| 网络 API | `network-api/api-v0/` | 后端无关的 socket 语义类型：端点、配置、状态机、错误与快照。 |
| smoltcp 实现 | `network-impl/impl-smoltcp/` | 基于 smoltcp 的 IPv4 协议栈：网卡适配、socket、TCP/UDP、poll 与初始化。 |
| socket 桥接 | `src/socket/` | `SocketRef` 共享句柄、`SocketReceiveLease` 接收预约、`VfsIoHandle` fd 适配。 |

## 实现说明

- `network-api` 不依赖 smoltcp、网卡驱动、VFS 或 syscall；具体协议栈通过这些类型向聚合层
  与 syscall 层报告 socket 状态和错误。
- `SocketState` 是内核侧跟踪的 socket 状态机（Created/Bound/Listening/Connecting/Connected/
  Closed），不是协议栈内部状态。
- `impl-smoltcp` 只通过 `stack` 模块暴露稳定调用面（`init`/`poll`/`poll_at_millis`/
  `poll_socket_events`/`network_socket_table_snapshot`）；创建/收发等 `socket_*` 辅助函数保持
  crate 私有，避免把内部细节暴露给上层。
- `vfs-handles` 把协议栈裸句柄封装为共享生命周期的 `SocketRef`，并通过 `into_vfs_handle`
  桥接到统一 VFS fd 表（`TcpSocketHandle`/`UdpSocketHandle` 实现 `VfsIoHandle`）。
- 接收采用预约模型：`prepare_receive` 产生 `SocketReceiveLease`，用户复制成功后 `finish`
  提交；非阻塞语义由上层用 `SocketRecvError`/`SocketSendError` 映射 Linux errno。
- 网卡通过 `adapter.rs` 连接 `wateros-driver` 的 `NetworkDevice`；驱动提供 L2 帧收发，协议栈
  负责 IP/TCP/UDP 语义。

## 调用链路

协议栈初始化（内核启动）：

```text
boot / bring-up
  -> stack::init(NetworkConfig{address, prefix_len, gateway})
  -> 建立 smoltcp 全局栈状态（global.rs）
```

socket 生命周期：

```text
sys_socket
  -> stack::create_tcp_socket / create_udp_socket
  -> SocketRef（object.rs）
  -> into_vfs_handle -> TcpSocketHandle / UdpSocketHandle（fd.rs，实现 VfsIoHandle）
  -> socket_bind / socket_listen / socket_connect / socket_accept
```

收发与轮询：

```text
write / read
  -> SocketRef::send / send_to
  -> SocketRef::prepare_receive -> SocketReceiveLease -> finish
poll / select
  -> stack::poll_at_millis / poll_socket_events / socket_poll_snapshot
```

## 各实现功能

### network-api / 网络语义 API

`network-api/api-v0/src/lib.rs`：

- `Ipv4Endpoint { address, port }`：IPv4 地址与端口。
- `NetworkConfig { address, prefix_len, gateway }`：协议栈初始化配置。
- `NetworkError`：后端无关失败原因，`NetworkResult<T>` 别名。
- `SocketKind`（`Tcp` / `Udp`）、`SocketState`（内核侧状态机）。
- `NetworkSocketSnapshot`：`/proc/net` 等只读管理接口的快照。
- `SocketConnectError` / `SocketSendError` / `SocketRecvError` / `SocketPollSnapshot` /
  `SocketRecvFinish`：syscall 层据此映射稳定 Linux errno。

### impl-smoltcp / smoltcp 协议栈

`network-impl/impl-smoltcp/src/` 下的文件：

- `adapter.rs`：网卡适配——把 `wateros-driver` 的 `NetworkDevice` 接到 smoltcp 设备接口。
- `lib.rs`：模块聚合（`adapter` + `stack`）。
- `stack/init.rs`：协议栈初始化。
- `stack/global.rs`：全局协议栈状态。
- `stack/state.rs`：socket 状态管理。
- `stack/socket.rs`：socket 句柄与创建/关闭。
- `stack/tcp.rs` / `stack/udp.rs`：TCP / UDP 收发与连接语义。
- `stack/sockopt.rs`：`setsockopt` / `getsockopt`。
- `stack/poll.rs`：`poll` / `poll_at_millis` / `poll_socket_events`。
- `stack/receive.rs`：接收预约与完成。
- `stack/types.rs`：内部类型。
- `stack/mod.rs`：稳定调用面（`init` / `poll` / `poll_at_millis` / `poll_socket_events` /
  `network_socket_table_snapshot`）。

### socket 桥接 / src/socket/

- `object.rs`：`SocketRef` 共享句柄（TCP/UDP 创建、bind/connect/listen/accept、send/sendto、
  sockopt、poll_snapshot），`NEXT_SOCKET_INODE` 分配稳定 inode。
- `lease.rs`：`SocketReceiveLease`——接收预约的稳定字节视图，`finish` 提交。
- `fd.rs`：`into_vfs_handle` / `from_vfs_handle` 与 `TcpSocketHandle` / `UdpSocketHandle`
  （实现 `VfsIoHandle`）的 fd 适配。
- `mod.rs`：重导出 `SocketRef` 与 `SocketReceiveLease`。

### 聚合门面 / src/lib.rs

- `pub use api_v0 as api`：后端无关 socket 语义类型。
- `stack`：当前活动协议栈的稳定调用面。
- `vfs-handles`：`SocketRef` / `SocketReceiveLease` 对上层可见。

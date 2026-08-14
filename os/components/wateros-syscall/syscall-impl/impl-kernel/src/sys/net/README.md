# net syscall

本目录把 Linux socket ABI 适配到 `wateros-network`，fd/阻塞公共机制位于
`socket_fd.rs`、`socket_block.rs` 和 `unix_sock.rs`。

## 当前能力

- IPv4 TCP/UDP socket、bind/connect/listen/accept、send/recv、sendmsg/recvmsg/recvmmsg。
- getsockname/getpeername、常用 socket option、shutdown 与 poll/epoll。
- AF_UNIX stream/socketpair、pathname 端点、accept 队列和 SCM 基础路径。

## 已知边界

当前重点是 QEMU IPv4 与本地 AF_UNIX；IPv6、raw/netlink/packet socket、完整 ancillary
data、零拷贝和复杂 TCP option 依赖网络后端扩展。未知协议族、类型和 option 返回
`EAFNOSUPPORT/EPROTONOSUPPORT/EOPNOTSUPP`，不得创建假 socket。

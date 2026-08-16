//! smoltcp 协议栈公共门面。
//!
//! 在设备驱动 `init_after_boot` 完成网卡注册后调用 [`init`]，
//! 之后通过周期性 [`poll`] 驱动协议栈。

mod types;
mod state;
mod global;
mod init;
mod poll;
mod socket;
mod receive;
mod tcp;
mod udp;
mod sockopt;

pub use init::init;
pub use poll::{poll, poll_at_millis, poll_socket_events};
pub use receive::{socket_finish_recv, socket_prepare_recv, SocketRecvReservation};
pub use socket::{
    network_socket_table_snapshot, socket_bind, socket_close, socket_connect, socket_kind,
    socket_local_endpoint, socket_peer_endpoint, socket_peer_is_loopback, socket_poll_snapshot,
    socket_send, socket_shutdown,
};
pub use sockopt::{socket_getsockopt, socket_recv_timeout_ms, socket_setsockopt};
pub use tcp::{create_tcp_socket, socket_accept, socket_listen};
pub use types::{
    NetworkError, NetworkSocketSnapshot, SocketConnectError, SocketKind, SocketRecvError,
    SocketRecvFinish, SocketSendError, SocketState, StackSocketHandle,
};
pub use udp::{create_udp_socket, socket_sendto};

//! smoltcp 协议栈公共门面。
//!
//! 在设备驱动 `init_after_boot` 完成网卡注册后调用 [`init`]，
//! 之后通过周期性 [`poll`] 驱动协议栈。

mod init;
mod poll;
mod socket;
mod sockopt;
mod state;
mod tcp;
mod types;
mod udp;

pub use init::init;
pub use poll::{poll, poll_at_millis, poll_socket_events};
pub use socket::{
    socket_bind, socket_close, socket_connect, socket_finish_recv, socket_kind,
    socket_local_endpoint, socket_local_port, socket_may_send, socket_peer_endpoint,
    socket_peer_is_loopback, socket_peername, socket_poll_snapshot, socket_prepare_recv,
    socket_recv, socket_send, socket_send_capacity, socket_shutdown, socket_state,
    SocketRecvReservation,
};
pub use sockopt::{socket_getsockopt, socket_recv_timeout_ms, socket_setsockopt};
pub use tcp::{
    create_tcp_socket, socket_accept, socket_can_recv, socket_is_connected, socket_listen,
    socket_may_recv, with_tcp_socket,
};
pub use types::{
    Ipv4Endpoint, NetworkConfig, NetworkError, NetworkResult, SocketKind, SocketPollSnapshot,
    SocketRecvError, SocketRecvFinish, SocketSendError, SocketState, StackSocketHandle,
};
pub use udp::{
    create_udp_socket, socket_recvfrom, socket_sendto, socket_udp_can_recv, with_udp_socket,
};

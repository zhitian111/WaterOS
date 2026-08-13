//! WaterOS 网络协议栈聚合层。
//!
//! [`api`] 提供后端无关的 socket 语义类型；启用 `impl-smoltcp` 时，
//! [`stack`] 提供当前活动协议栈；`vfs-handles` 负责 socket 与 VFS fd 的桥接。

#![no_std]
extern crate alloc;

pub use api_v0 as api;
pub use api_v0::{
    Ipv4Endpoint, NetworkConfig, NetworkError, NetworkResult, NetworkSocketSnapshot,
    SocketConnectError, SocketKind, SocketPollSnapshot, SocketRecvError, SocketRecvFinish,
    SocketSendError, SocketState,
};

#[cfg(feature = "impl-smoltcp")]
pub mod stack {
    //! 当前活动协议栈的稳定调用面。
    //!
    //! 这里选择性转发 syscall、VFS 和内核启动路径实际需要的能力，避免把
    //! `impl-smoltcp` 的内部辅助函数整体暴露给上层。

    pub use impl_smoltcp::stack::{
        init, network_socket_table_snapshot, poll, poll_at_millis, poll_socket_events,
    };

    pub(crate) use impl_smoltcp::stack::{
        create_tcp_socket, create_udp_socket, socket_accept, socket_bind, socket_close,
        socket_connect, socket_finish_recv, socket_getsockopt, socket_kind, socket_listen,
        socket_local_endpoint, socket_peer_endpoint, socket_peer_is_loopback, socket_poll_snapshot,
        socket_prepare_recv, socket_recv_timeout_ms, socket_send, socket_sendto, socket_setsockopt,
        socket_shutdown, SocketRecvReservation, StackSocketHandle,
    };
}

#[cfg(feature = "vfs-handles")]
mod socket;

#[cfg(feature = "vfs-handles")]
pub use socket::{SocketReceiveLease, SocketRef};

/// 网络组件内核态自检：验证协议栈门面可用，不启动用户态任务或真实连接。
#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[network] self_test begin");
    let config = NetworkConfig { address: [10, 0, 2, 15], prefix_len: 24, gateway: [10, 0, 2, 2] };
    assert_eq!(config.prefix_len, 24);
    assert_ne!(config.address, config.gateway);
    #[cfg(feature = "impl-smoltcp")]
    impl_smoltcp::self_test();
    log::info!("[network] self_test complete");
}

//! WaterOS 网络协议栈聚合层。
//!
//! [`api`] 提供后端无关的 socket 语义类型；启用 `impl-smoltcp` 时，
//! [`stack`] 提供当前活动协议栈；`vfs-handles` 负责 socket 与 VFS fd 的桥接。

#![no_std]
extern crate alloc;

pub use api_v0 as api;
pub use api_v0::{
    Ipv4Endpoint, NetworkConfig, NetworkError, NetworkResult, SocketKind, SocketPollSnapshot,
    SocketRecvError, SocketRecvFinish, SocketSendError, SocketState,
};

#[cfg(feature = "impl-smoltcp")]
pub use impl_smoltcp::stack;

#[cfg(feature = "vfs-handles")]
pub mod socket_handles;

#[cfg(feature = "vfs-handles")]
pub use socket_handles::{SocketReceiveLease, SocketRef, TcpSocketHandle, UdpSocketHandle};

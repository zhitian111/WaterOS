//! WaterOS 网络协议栈的后端无关语义类型。
//!
//! 本 crate 不依赖 smoltcp、网卡驱动、VFS 或 syscall；具体协议栈实现通过
//! 这些类型向聚合层和 syscall 层报告 socket 状态与错误。

#![no_std]

use core::fmt;

/// IPv4 地址和端口的后端无关表示。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ipv4Endpoint {
    pub address: [u8; 4],
    pub port: u16,
}

/// IPv4 协议栈初始化配置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetworkConfig {
    pub address: [u8; 4],
    pub prefix_len: u8,
    pub gateway: [u8; 4],
}

/// 协议栈通用操作失败原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkError {
    StackUnavailable,
    AlreadyInitialized,
    InvalidSocket,
    WrongSocketType,
    InvalidState,
    InvalidArgument,
    AddressNotAvailable,
    AddressInUse,
    NotBound,
    NotConnected,
    NotListening,
    NoPendingConnection,
    ConnectionRefused,
    Unsupported,
    Internal,
    Io,
}

pub type NetworkResult<T> = Result<T, NetworkError>;

impl fmt::Display for NetworkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::StackUnavailable => "network stack unavailable",
            Self::AlreadyInitialized => "network stack already initialized",
            Self::InvalidSocket => "invalid socket",
            Self::WrongSocketType => "wrong socket type",
            Self::InvalidState => "invalid socket state",
            Self::InvalidArgument => "invalid argument",
            Self::AddressNotAvailable => "address not available",
            Self::AddressInUse => "address in use",
            Self::NotBound => "socket not bound",
            Self::NotConnected => "socket not connected",
            Self::NotListening => "socket not listening",
            Self::NoPendingConnection => "no pending connection",
            Self::ConnectionRefused => "connection refused",
            Self::Unsupported => "operation unsupported",
            Self::Internal => "network stack internal error",
            Self::Io => "network I/O error",
        };
        f.write_str(message)
    }
}

/// 协议栈支持的 socket 类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketKind {
    Tcp,
    Udp,
}

/// Socket 状态机（内核侧跟踪，非具体协议栈的内部状态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketState {
    Created,
    Bound { port: u16 },
    Listening { port: u16 },
    Connecting,
    Connected,
    Closed,
}

/// socket 发送失败原因；syscall 层据此返回稳定的 Linux errno。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketSendError {
    MessageTooLarge,
    WouldBlock,
    NoBufferSpace,
    NotConnected,
    InvalidDestination,
    InvalidSocket,
    StackUnavailable,
    Io,
}

/// 一次协议栈临界区内取得的 socket 就绪状态。
#[derive(Clone, Copy, Debug)]
pub struct SocketPollSnapshot {
    pub kind: SocketKind,
    pub state: SocketState,
    pub can_recv: bool,
    pub may_recv: bool,
    pub may_send: bool,
    pub send_capacity: usize,
    pub is_connected: bool,
    pub has_pending_accept: bool,
}

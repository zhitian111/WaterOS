//! WaterOS 网络协议栈的后端无关语义类型。
//!
//! 本 crate 不依赖 smoltcp、网卡驱动、VFS 或 syscall；具体协议栈实现通过
//! 这些类型向聚合层和 syscall 层报告 socket 状态与错误。

#![no_std]

use core::fmt;

/// IPv4 地址和端口的后端无关表示。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ipv4Endpoint {
    pub address : [u8; 4],
    pub port : u16,
}

/// IPv4 协议栈初始化配置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetworkConfig {
    pub address : [u8; 4],
    pub prefix_len : u8,
    pub gateway : [u8; 4],
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
    fn fmt(&self, f : &mut fmt::Formatter<'_>) -> fmt::Result {
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
    Bound { port : u16 },
    Listening { port : u16 },
    Connecting,
    Connected,
    Closed,
}

/// `/proc/net` 等只读管理接口需要的 socket 状态快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetworkSocketSnapshot {
    pub kind : SocketKind,
    pub state : SocketState,
    pub local : Ipv4Endpoint,
    pub peer : Ipv4Endpoint,
    pub tx_queue : usize,
    pub rx_queue : usize,
}

/// 非阻塞 TCP 连接完成后，通过 `SO_ERROR` 交给用户态的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketConnectError {
    ConnectionRefused,
    TimedOut,
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

/// 接收预留的创建或提交错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketRecvError {
    Busy,
    Empty,
    Finished,
    InvalidSocket,
    NoMemory,
    Io,
}

/// 提交或取消一次接收预留的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketRecvFinish {
    Bytes(usize),
    Fault,
}

/// 一次协议栈临界区内取得的 socket 就绪状态。
#[derive(Clone, Copy, Debug)]
pub struct SocketPollSnapshot {
    pub kind : SocketKind,
    pub state : SocketState,
    pub can_recv : bool,
    pub may_recv : bool,
    pub may_send : bool,
    /// 当前发送缓冲区还能接收的字节数，不是缓冲区总容量。
    pub send_capacity : usize,
    pub is_connected : bool,
    /// 异步 connect 失败时保留到用户态读取 `SO_ERROR`。
    pub connect_error : Option<SocketConnectError>,
    pub has_pending_accept : bool,
}

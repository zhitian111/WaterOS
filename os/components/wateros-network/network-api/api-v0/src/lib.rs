//! WaterOS 网络协议栈的后端无关语义类型。
//!
//! 本 crate 不依赖 smoltcp、网卡驱动、VFS 或 syscall；具体协议栈实现通过
//! 这些类型向聚合层和 syscall 层报告 socket 状态与错误。

#![no_std]

/// IPv4 地址和端口的后端无关表示。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ipv4Endpoint {
    /// IPv4 地址，按网络字节序拆成四个八位组。
    pub address : [u8; 4],
    /// TCP/UDP 端口号，取值范围为 0..=65535；端口 0 的绑定语义由后端决定。
    pub port : u16,
}

/// IPv4 协议栈初始化配置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetworkConfig {
    /// 本机 IPv4 地址。
    pub address : [u8; 4],
    /// CIDR 前缀长度，必须不超过 32。
    pub prefix_len : u8,
    /// 默认网关 IPv4 地址；无网关时由实现使用零地址或拒绝配置。
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
    /// socket 协议类型。
    pub kind : SocketKind,
    /// 内核 socket 状态机状态。
    pub state : SocketState,
    /// 本地端点；未绑定时通常为零地址/端口。
    pub local : Ipv4Endpoint,
    /// 对端端点；未连接时通常为零地址/端口。
    pub peer : Ipv4Endpoint,
    /// 尚未交给协议栈发送完成的字节数。
    pub tx_queue : usize,
    /// 已到达但尚未被用户读取的字节数。
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
    /// socket 类型。
    pub kind : SocketKind,
    /// 当前状态。
    pub state : SocketState,
    /// 立即读取不会阻塞或返回数据的条件。
    pub can_recv : bool,
    /// 读取端已结束，通常对应 EOF/FIN。
    pub may_recv : bool,
    /// 当前允许至少提交一部分发送数据。
    pub may_send : bool,
    /// 当前发送缓冲区还能接收的字节数，不是缓冲区总容量。
    pub send_capacity : usize,
    /// 是否已完成连接握手。
    pub is_connected : bool,
    /// 异步 connect 失败时保留到用户态读取 `SO_ERROR`。
    pub connect_error : Option<SocketConnectError>,
    /// 监听队列中是否存在可接受连接。
    pub has_pending_accept : bool,
}

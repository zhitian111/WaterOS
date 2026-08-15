//! WaterOS 网络协议栈的后端无关语义类型。
//!
//! 本 crate 不依赖 smoltcp、网卡驱动、VFS 或 syscall；具体协议栈实现通过
//! 这些类型向聚合层和 syscall 层报告 socket 状态与错误。

#![no_std]

/// 网络层地址的后端无关表示。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkAddress {
    Ipv4([u8; 4]),
    Ipv6([u8; 16]),
}

impl NetworkAddress {
    pub const fn unspecified(domain : SocketDomain) -> Self {
        match domain {
            SocketDomain::Ipv4 => Self::Ipv4([0; 4]),
            SocketDomain::Ipv6 => Self::Ipv6([0; 16]),
        }
    }

    pub fn is_unspecified(self) -> bool {
        match self {
            Self::Ipv4(address) => address == [0; 4],
            Self::Ipv6(address) => address == [0; 16],
        }
    }

    pub fn is_loopback(self) -> bool {
        match self {
            Self::Ipv4(address) => address[0] == 127,
            Self::Ipv6(address) => address == [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1],
        }
    }

    pub const fn domain(self) -> SocketDomain {
        match self {
            Self::Ipv4(_) => SocketDomain::Ipv4,
            Self::Ipv6(_) => SocketDomain::Ipv6,
        }
    }
}

/// IP 地址、端口及 IPv6 scope id。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetworkEndpoint {
    pub address : NetworkAddress,
    pub port : u16,
    pub scope_id : u32,
}

/// 静态 IPv6 协议栈初始化配置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ipv6Config {
    pub address : [u8; 16],
    pub prefix_len : u8,
    pub gateway : [u8; 16],
}

/// IPv4 与可选静态 IPv6 协议栈初始化配置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetworkConfig {
    pub address : [u8; 4],
    pub prefix_len : u8,
    pub gateway : [u8; 4],
    pub ipv6 : Option<Ipv6Config>,
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

/// Internet socket 使用的地址族。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketDomain {
    Ipv4,
    Ipv6,
}

/// 协议栈支持的 socket 类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SocketKind {
    Tcp,
    Udp,
    /// Echo-only raw ICMP/ICMPv6 socket used by ping.
    Icmp,
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
    pub domain : SocketDomain,
    pub kind : SocketKind,
    pub state : SocketState,
    pub local : NetworkEndpoint,
    pub peer : NetworkEndpoint,
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

#[cfg(test)]
mod tests {
    use super::{NetworkAddress, SocketDomain};

    #[test]
    fn unspecified_addresses_keep_their_domain() {
        let ipv4 = NetworkAddress::unspecified(SocketDomain::Ipv4);
        let ipv6 = NetworkAddress::unspecified(SocketDomain::Ipv6);
        assert!(ipv4.is_unspecified());
        assert!(ipv6.is_unspecified());
        assert_eq!(ipv4.domain(), SocketDomain::Ipv4);
        assert_eq!(ipv6.domain(), SocketDomain::Ipv6);
    }

    #[test]
    fn loopback_detection_covers_both_families() {
        assert!(NetworkAddress::Ipv4([127, 0, 0, 42]).is_loopback());
        assert!(NetworkAddress::Ipv6([0, 0, 0, 0, 0, 0, 0, 0,
                                      0, 0, 0, 0, 0, 0, 0, 1]).is_loopback());
        assert!(!NetworkAddress::Ipv6([0; 16]).is_loopback());
    }
}

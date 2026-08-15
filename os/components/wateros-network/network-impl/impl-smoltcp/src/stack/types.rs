//! 协议栈对外共享的 socket 类型。

use smoltcp::iface::SocketHandle;

pub use api_v0::{
    NetworkAddress, NetworkConfig, NetworkEndpoint, NetworkError, NetworkResult,
    NetworkSocketSnapshot, SocketConnectError, SocketDomain, SocketKind, SocketPollSnapshot,
    SocketRecvError, SocketRecvFinish, SocketSendError, SocketState,
};

/// 对外隐藏具体导入路径的协议栈 socket 句柄。
pub type StackSocketHandle = SocketHandle;

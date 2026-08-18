//! 协议栈对外共享的 socket 类型。

use smoltcp::iface::SocketHandle;

pub use api_v0::{
    Ipv4Endpoint, NetworkConfig, NetworkError, NetworkResult, NetworkSocketSnapshot,
    SocketConnectError, SocketKind, SocketPollSnapshot, SocketRecvError, SocketRecvFinish,
    SocketSendError, SocketState,
};

/// 对外隐藏具体导入路径的协议栈 socket 句柄。
///
/// 句柄只在协议栈锁保护的生命周期内有效；关闭 socket 后不得继续使用，
/// 因为底层 `SocketSet` 可能复用该编号。
pub type StackSocketHandle = SocketHandle;

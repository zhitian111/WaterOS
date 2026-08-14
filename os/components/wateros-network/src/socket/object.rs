//! Socket 内核对象：负责对象能力、共享生命周期和底层句柄串行化。

use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use spin::Mutex;

use crate::stack::{self, StackSocketHandle};
use crate::{
    Ipv4Endpoint, NetworkResult, SocketKind, SocketPollSnapshot, SocketSendError,
};

static NEXT_SOCKET_INODE : AtomicU64 = AtomicU64::new(1);

/// 同一打开 socket 的共享状态；`dup`/`fork` 产生的 fd 与在途 syscall 共同持有。
struct SocketShared {
    handle : Mutex<StackSocketHandle>,
    status_flags : AtomicUsize,
}

impl Drop for SocketShared {
    fn drop(&mut self) {
        // 所有 fd 和正在执行的 syscall 都已释放引用，此处恰好关闭一次底层 socket。
        let handle = *self.handle
                          .get_mut();
        if let Err(err) = stack::socket_close(handle) {
            log::warn!("[socket-ref] final close failed handle={:?} err={:?}",
                       handle,
                       err);
        }
    }
}

/// WaterOS 网络 socket 的共享引用，也是 syscall 使用的主要对象接口。
#[derive(Clone)]
pub struct SocketRef {
    inner : Arc<SocketShared>,
    inode : u64,
}

impl SocketRef {
    /// 创建 TCP socket，并把底层句柄封装进共享生命周期对象。
    pub fn new_tcp(status_flags : usize) -> NetworkResult<Self> {
        let handle = stack::create_tcp_socket()?;
        Ok(Self::from_stack_handle(handle, status_flags))
    }

    /// 创建 UDP socket，并把底层句柄封装进共享生命周期对象。
    pub fn new_udp(status_flags : usize) -> NetworkResult<Self> {
        let handle = stack::create_udp_socket()?;
        Ok(Self::from_stack_handle(handle, status_flags))
    }

    /// 仅供本模块包装新建或 accept 得到的底层句柄。
    fn from_stack_handle(handle : StackSocketHandle, status_flags : usize) -> Self {
        Self { inner : Arc::new(SocketShared { handle : Mutex::new(handle),
                                               status_flags : AtomicUsize::new(status_flags) }),
               inode : NEXT_SOCKET_INODE.fetch_add(1, Ordering::Relaxed) }
    }

    /// 在底层句柄保持稳定期间执行一次协议栈操作。
    pub(super) fn with_handle<T>(&self, operation : impl FnOnce(StackSocketHandle) -> T) -> T {
        let handle = self.inner
                         .handle
                         .lock();
        operation(*handle)
    }

    /// 原子完成 accept 与监听句柄置换，串行化同一监听 fd 上的并发 accept。
    pub fn accept(&self, status_flags : usize) -> NetworkResult<(SocketRef, Ipv4Endpoint)> {
        let mut listener = self.inner
                               .handle
                               .lock();
        let (established, replacement, peer_ip, peer_port) = stack::socket_accept(*listener)?;
        *listener = replacement;
        Ok((Self::from_stack_handle(established, status_flags),
            Ipv4Endpoint { address : peer_ip,
                           port : peer_port }))
    }

    pub fn kind(&self) -> NetworkResult<SocketKind> { self.with_handle(stack::socket_kind) }

    pub fn bind(&self, local_ip : Option<[u8; 4]>, port : u16) -> NetworkResult<()> {
        self.with_handle(|handle| stack::socket_bind(handle, local_ip, port))
    }

    pub fn connect(&self, endpoint : Ipv4Endpoint) -> NetworkResult<()> {
        self.with_handle(|handle| {
            stack::socket_connect(handle, endpoint.address, endpoint.port)
        })
    }

    pub fn listen(&self, backlog : usize) -> NetworkResult<()> {
        self.with_handle(|handle| stack::socket_listen(handle, backlog))
    }

    pub fn shutdown(&self) -> NetworkResult<()> { self.with_handle(stack::socket_shutdown) }

    pub fn local_endpoint(&self) -> NetworkResult<Ipv4Endpoint> {
        self.with_handle(stack::socket_local_endpoint)
    }

    pub fn peer_endpoint(&self) -> NetworkResult<Ipv4Endpoint> {
        self.with_handle(stack::socket_peer_endpoint)
    }

    pub fn peer_is_loopback(&self) -> NetworkResult<bool> {
        self.with_handle(stack::socket_peer_is_loopback)
    }

    pub fn poll_snapshot(&self) -> NetworkResult<SocketPollSnapshot> {
        self.with_handle(stack::socket_poll_snapshot)
    }

    pub fn send(&self, data : &[u8]) -> Result<usize, SocketSendError> {
        self.with_handle(|handle| stack::socket_send(handle, data))
    }

    pub fn send_to(&self, data : &[u8], endpoint : Ipv4Endpoint) -> Result<usize, SocketSendError> {
        self.with_handle(|handle| {
            stack::socket_sendto(handle, data, endpoint.address, endpoint.port)
        })
    }

    pub fn set_sockopt(&self, level : usize, optname : usize, value : &[u8]) -> NetworkResult<()> {
        self.with_handle(|handle| stack::socket_setsockopt(handle, level, optname, value))
    }

    pub fn get_sockopt(&self, level : usize, optname : usize) -> NetworkResult<Vec<u8>> {
        self.with_handle(|handle| stack::socket_getsockopt(handle, level, optname))
    }

    pub fn recv_timeout_ms(&self) -> NetworkResult<Option<u64>> {
        self.with_handle(stack::socket_recv_timeout_ms)
    }

    pub fn status_flags(&self) -> usize {
        self.inner
            .status_flags
            .load(Ordering::Acquire)
    }

    pub fn set_status_flags(&self, flags : usize) {
        self.inner
            .status_flags
            .store(flags, Ordering::Release);
    }

    pub(super) fn inode(&self) -> u64 { self.inode }
}

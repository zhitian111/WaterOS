//! Socket 文件描述符句柄：将 smoltcp [`SocketHandle`] 桥接到 VFS [`VfsIoHandle`] trait。
//!
//! 仅在 `impl-smoltcp` feature 启用时编译。

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use smoltcp::iface::SocketHandle;
use spin::Mutex;
use vfs_api::error::{VfsError, VfsResult};
use vfs_api::handle::{
    VfsCopyProgress, VfsIoHandle, VfsPreparedRead, VfsReadFinish, VfsReadLease,
};
use vfs_api::meta::{VfsMetadata, VfsNodeType};

use crate::stack;

static NEXT_SOCKET_INODE: AtomicU64 = AtomicU64::new(1);
const POLLIN: i16 = 0x001;
const POLLOUT: i16 = 0x004;
const POLLHUP: i16 = 0x010;

/// 为 socket fd 分配伪 inode 的 VFS 元数据（特殊字符设备形态）。
fn socket_meta(inode: u64) -> VfsMetadata {
    VfsMetadata {
        node_type: VfsNodeType::Special,
        size: 0,
        mode: 0o140777, // srwxrwxrwx
        device_major: 0,
        device_minor: 0x7fff_0002,
        inode,
        mount_id: 0,
        nlink: 1,
        uid: 0,
        gid: 0,
    }
}

/// 同一打开 socket 的共享状态；`dup`/`fork` 产生的 fd 与在途 syscall 共同持有。
struct SocketShared {
    handle: Mutex<SocketHandle>,
    status_flags: AtomicUsize,
}

impl Drop for SocketShared {
    fn drop(&mut self) {
        // 所有 fd 和正在执行的 syscall 都已释放引用，此处恰好关闭一次底层 socket。
        let handle = *self.handle.get_mut();
        if let Err(err) = stack::socket_close(handle) {
            log::warn!("[socket-ref] final close failed handle={:?} err={}", handle, err);
        }
    }
}

#[derive(Clone)]
pub struct SocketRef {
    inner: Arc<SocketShared>,
    inode: u64,
}

impl SocketRef {
    /// 包装 smoltcp 句柄并分配唯一伪 inode。
    pub fn new(handle: SocketHandle) -> Self {
        Self::new_with_status_flags(handle, 0)
    }

    /// 包装 smoltcp 句柄，并设置共享的打开状态标志（如 `O_NONBLOCK`）。
    pub fn new_with_status_flags(handle: SocketHandle, status_flags: usize) -> Self {
        Self {
            inner: Arc::new(SocketShared {
                handle: Mutex::new(handle),
                status_flags: AtomicUsize::new(status_flags),
            }),
            inode: NEXT_SOCKET_INODE.fetch_add(1, Ordering::Relaxed),
        }
    }

    /// 读取当前 smoltcp 句柄（短暂持锁）。
    pub fn handle(&self) -> SocketHandle {
        *self.inner.handle.lock()
    }

    /// 原子完成 accept 与监听句柄置换，串行化同一监听 fd 上的并发 accept。
    pub fn accept(&self) -> Result<(SocketHandle, [u8; 4], u16), &'static str> {
        let mut listener = self.inner.handle.lock();
        let (established, replacement, peer_ip, peer_port) = stack::socket_accept(*listener)?;
        *listener = replacement;
        Ok((established, peer_ip, peer_port))
    }

    pub fn status_flags(&self) -> usize {
        self.inner.status_flags.load(Ordering::Acquire)
    }

    pub fn set_status_flags(&self, flags: usize) {
        self.inner.status_flags.store(flags, Ordering::Release);
    }

    /// Reserve received bytes while retaining this socket's lifetime.
    pub fn prepare_receive(&self, max_len: usize) -> Result<SocketReceiveLease, stack::SocketRecvError> {
        let snapshot = stack::socket_poll_snapshot(self.handle())
            .map_err(|_| stack::SocketRecvError::InvalidSocket)?;
        if !snapshot.can_recv {
            if snapshot.kind == stack::SocketKind::Tcp && !snapshot.may_recv {
                return Err(stack::SocketRecvError::Finished);
            }
            return Err(stack::SocketRecvError::Empty);
        }
        let mut data = Vec::new();
        data.try_reserve_exact(max_len)
            .map_err(|_| stack::SocketRecvError::NoMemory)?;
        data.resize(max_len, 0);
        let reservation = stack::socket_prepare_recv(self.handle(), &mut data)?;
        data.truncate(reservation.staged_len());
        Ok(SocketReceiveLease {
            _socket: self.clone(),
            reservation: Some(reservation),
            data,
        })
    }

    fn inode(&self) -> u64 {
        self.inode
    }
}

/// Owned receive reservation shared by read, recvfrom and recvmsg.
pub struct SocketReceiveLease {
    _socket: SocketRef,
    reservation: Option<stack::SocketRecvReservation>,
    data: Vec<u8>,
}

impl SocketReceiveLease {
    pub fn bytes(&self) -> &[u8] { self.data.as_slice() }

    pub fn source(&self) -> ([u8; 4], u16) {
        self.reservation.as_ref()
                        .map(stack::SocketRecvReservation::source)
                        .unwrap_or(([0; 4], 0))
    }

    pub fn kind(&self) -> stack::SocketKind {
        self.reservation.as_ref()
                        .map(stack::SocketRecvReservation::kind)
                        .unwrap_or(stack::SocketKind::Tcp)
    }

    pub fn datagram_len(&self) -> usize {
        self.reservation.as_ref()
                        .map(stack::SocketRecvReservation::datagram_len)
                        .unwrap_or(0)
    }

    pub fn finish(mut self, copied: usize, complete: bool)
                  -> Result<stack::SocketRecvFinish, stack::SocketRecvError> {
        let reservation = self.reservation.take().ok_or(stack::SocketRecvError::Io)?;
        stack::socket_finish_recv(reservation, copied, complete)
    }
}

impl Drop for SocketReceiveLease {
    fn drop(&mut self) {
        if let Some(reservation) = self.reservation.take() {
            let _ = stack::socket_finish_recv(reservation, 0, false);
        }
    }
}

struct SocketPreparedRead {
    socket: SocketRef,
    max_len: usize,
}

impl VfsPreparedRead for SocketPreparedRead {
    fn acquire(self: Box<Self>) -> VfsResult<Box<dyn VfsReadLease>> {
        match self.socket.prepare_receive(self.max_len) {
            Ok(lease) => Ok(Box::new(SocketVfsReadLease { lease: Some(lease) })),
            Err(stack::SocketRecvError::Finished) => Ok(Box::new(EmptySocketReadLease)),
            Err(stack::SocketRecvError::Busy | stack::SocketRecvError::Empty) => {
                const O_NONBLOCK: usize = 0o4000;
                if self.socket.status_flags() & O_NONBLOCK != 0 {
                    Err(VfsError::WouldBlock)
                } else {
                    Err(VfsError::Busy)
                }
            }
            Err(error) => Err(map_recv_stack_err(error)),
        }
    }
}

struct EmptySocketReadLease;

impl VfsReadLease for EmptySocketReadLease {
    fn bytes(&self) -> &[u8] { &[] }

    fn finish(self: Box<Self>, progress: VfsCopyProgress) -> VfsResult<VfsReadFinish> {
        if progress.copied != 0 {
            return Err(VfsError::Io);
        }
        Ok(VfsReadFinish::Bytes(0))
    }
}

struct SocketVfsReadLease {
    lease: Option<SocketReceiveLease>,
}

impl VfsReadLease for SocketVfsReadLease {
    fn bytes(&self) -> &[u8] {
        self.lease.as_ref().map(SocketReceiveLease::bytes).unwrap_or(&[])
    }

    fn finish(mut self: Box<Self>, progress: VfsCopyProgress) -> VfsResult<VfsReadFinish> {
        match self.lease.take()
                        .ok_or(VfsError::Io)?
                        .finish(progress.copied, progress.complete)
                        .map_err(map_recv_stack_err)? {
            stack::SocketRecvFinish::Bytes(copied) => Ok(VfsReadFinish::Bytes(copied)),
            stack::SocketRecvFinish::Fault => Ok(VfsReadFinish::Fault),
        }
    }
}

/// TCP 已连接 socket 的 fd 句柄。
pub struct TcpStreamHandle {
    pub socket: SocketRef,
}

impl VfsIoHandle for TcpStreamHandle {
    fn open_accmode(&self) -> u32 { 2 }

    fn open_status_flags(&self) -> u32 { self.socket.status_flags() as u32 }

    fn set_open_status_flags(&mut self, flags : u32) -> VfsResult<()> {
        const O_NONBLOCK : usize = 0o4000;
        self.socket.set_status_flags(flags as usize & O_NONBLOCK);
        Ok(())
    }

    fn prepare_read(&mut self, max_len: usize) -> VfsResult<Box<dyn VfsPreparedRead>> {
        Ok(Box::new(SocketPreparedRead { socket: self.socket.clone(), max_len }))
    }

    fn read(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        read_with_lease(&self.socket, buf)
    }

    fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        stack::socket_send(self.socket.handle(), buf).map_err(map_send_stack_err)
    }

    fn poll_revents(&mut self, events: i16) -> VfsResult<i16> {
        let handle = self.socket.handle();
        let snapshot = stack::socket_poll_snapshot(handle).map_err(map_stack_err)?;
        let mut revents = 0;
        if events & POLLIN != 0 {
            if snapshot.can_recv || !snapshot.may_recv {
                revents |= POLLIN;
            }
        }
        if events & POLLOUT != 0
            && snapshot.may_send
            && snapshot.send_capacity > 0
        {
            revents |= POLLOUT;
        }
        if snapshot.state == stack::SocketState::Closed {
            revents |= POLLHUP;
        }
        Ok(revents)
    }

    fn close(&mut self) -> VfsResult<()> {
        // 底层 socket 由 SocketShared::drop 在最后一个 fd/在途操作释放时关闭。
        Ok(())
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(socket_meta(self.socket.inode()))
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(Self {
            socket: self.socket.clone(),
        }))
    }
}

/// TCP 监听 socket 的 fd 句柄。
pub struct TcpListenerHandle {
    pub socket: SocketRef,
}

impl VfsIoHandle for TcpListenerHandle {
    fn open_accmode(&self) -> u32 { 2 }

    fn open_status_flags(&self) -> u32 { self.socket.status_flags() as u32 }

    fn set_open_status_flags(&mut self, flags : u32) -> VfsResult<()> {
        const O_NONBLOCK : usize = 0o4000;
        self.socket.set_status_flags(flags as usize & O_NONBLOCK);
        Ok(())
    }

    fn close(&mut self) -> VfsResult<()> {
        Ok(())
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(socket_meta(self.socket.inode()))
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(Self {
            socket: self.socket.clone(),
        }))
    }
}

/// UDP socket 的 fd 句柄。
pub struct UdpSocketHandle {
    pub socket: SocketRef,
}

impl VfsIoHandle for UdpSocketHandle {
    fn open_accmode(&self) -> u32 { 2 }

    fn open_status_flags(&self) -> u32 { self.socket.status_flags() as u32 }

    fn set_open_status_flags(&mut self, flags : u32) -> VfsResult<()> {
        const O_NONBLOCK : usize = 0o4000;
        self.socket.set_status_flags(flags as usize & O_NONBLOCK);
        Ok(())
    }

    fn prepare_read(&mut self, max_len: usize) -> VfsResult<Box<dyn VfsPreparedRead>> {
        Ok(Box::new(SocketPreparedRead { socket: self.socket.clone(), max_len }))
    }

    fn read(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        read_with_lease(&self.socket, buf)
    }

    fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        stack::socket_send(self.socket.handle(), buf).map_err(map_send_stack_err)
    }

    fn poll_revents(&mut self, events: i16) -> VfsResult<i16> {
        let handle = self.socket.handle();
        let snapshot = stack::socket_poll_snapshot(handle).map_err(map_stack_err)?;
        let mut revents = 0;
        if events & POLLIN != 0 && snapshot.can_recv {
            revents |= POLLIN;
        }
        if events & POLLOUT != 0
            && snapshot.may_send
            && snapshot.send_capacity > 0
        {
            revents |= POLLOUT;
        }
        Ok(revents)
    }

    fn close(&mut self) -> VfsResult<()> {
        Ok(())
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(socket_meta(self.socket.inode()))
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(Self {
            socket: self.socket.clone(),
        }))
    }
}

fn read_with_lease(socket: &SocketRef, buf: &mut [u8]) -> VfsResult<usize> {
    let lease = socket.prepare_receive(buf.len()).map_err(map_recv_stack_err)?;
    let len = lease.bytes().len();
    buf[..len].copy_from_slice(lease.bytes());
    match lease.finish(len, true).map_err(map_recv_stack_err)? {
        stack::SocketRecvFinish::Bytes(copied) => Ok(copied),
        stack::SocketRecvFinish::Fault => Err(VfsError::Io),
    }
}

/// 将协议栈 `&'static str` 错误映射为 VFS 错误码。
fn map_stack_err(err: &'static str) -> VfsError {
    match err {
        "no connected tcp socket" | "invalid socket handle" | "not a tcp socket" => VfsError::BadFd,
        "recv failed" | "send failed" | "udp recvfrom failed" => VfsError::Io,
        _ => VfsError::Unsupported,
    }
}

fn map_send_stack_err(err: stack::SocketSendError) -> VfsError {
    match err {
        stack::SocketSendError::WouldBlock => VfsError::WouldBlock,
        stack::SocketSendError::InvalidSocket | stack::SocketSendError::NotConnected => {
            VfsError::BadFd
        }
        stack::SocketSendError::MessageTooLarge
        | stack::SocketSendError::NoBufferSpace
        | stack::SocketSendError::InvalidDestination
        | stack::SocketSendError::StackUnavailable
        | stack::SocketSendError::Io => VfsError::Io,
    }
}

fn map_recv_stack_err(err: stack::SocketRecvError) -> VfsError {
    match err {
        stack::SocketRecvError::Busy => VfsError::Busy,
        stack::SocketRecvError::Empty => VfsError::WouldBlock,
        stack::SocketRecvError::Finished => VfsError::Unsupported,
        stack::SocketRecvError::InvalidSocket => VfsError::BadFd,
        stack::SocketRecvError::NoMemory => VfsError::NoMemory,
        stack::SocketRecvError::Io => VfsError::Io,
    }
}

//! Socket 的 fd 适配层：将 [`SocketRef`] 桥接到统一 VFS fd 表使用的句柄接口。

use alloc::boxed::Box;

use vfs_api::error::{VfsError, VfsResult};
use vfs_api::handle::{
    VfsCopyProgress, VfsIoHandle, VfsPreparedRead, VfsReadFinish, VfsReadLease, VfsResourceKind,
};
use vfs_api::meta::{VfsMetadata, VfsNodeType};

use crate::{
    NetworkError, NetworkResult, SocketKind, SocketRecvError, SocketRecvFinish, SocketSendError,
    SocketState,
};

use super::{SocketReceiveLease, SocketRef};

const POLLIN : i16 = 0x001;
const POLLOUT : i16 = 0x004;
const POLLHUP : i16 = 0x010;

impl SocketRef {
    /// 将 socket 所有权交给 VFS fd 表；具体 TCP/UDP 句柄类型保持为本模块实现细节。
    pub fn into_vfs_handle(self) -> NetworkResult<Box<dyn VfsIoHandle>> {
        match self.kind()? {
            SocketKind::Tcp => Ok(Box::new(TcpSocketHandle { socket : self })),
            SocketKind::Udp => Ok(Box::new(UdpSocketHandle { socket : self })),
        }
    }

    /// 从 VFS fd 句柄中识别并取得共享 socket 引用。
    pub fn from_vfs_handle(handle : &(dyn VfsIoHandle + '_)) -> Option<Self> {
        if let Some(handle) = handle.as_any().downcast_ref::<TcpSocketHandle>() {
            return Some(handle.socket.clone());
        }
        handle
            .as_any()
            .downcast_ref::<UdpSocketHandle>()
            .map(|handle| handle.socket.clone())
    }
}

/// 为 socket fd 分配伪 inode 的 VFS 元数据（特殊字符设备形态）。
fn socket_meta(inode : u64) -> VfsMetadata {
    VfsMetadata { node_type : VfsNodeType::Special,
                  size : 0,
                  mode : 0o140777, // srwxrwxrwx
                  device_major : 0,
                  device_minor : 0x7FFF_0002,
                  inode,
                  mount_id : 0,
                  nlink : 1,
                  uid : 0,
                  gid : 0 }
}

struct SocketPreparedRead {
    socket : SocketRef,
    max_len : usize,
}

impl VfsPreparedRead for SocketPreparedRead {
    fn acquire(self: Box<Self>) -> VfsResult<Box<dyn VfsReadLease>> {
        match self.socket
                  .prepare_receive(self.max_len)
        {
            Ok(lease) => Ok(Box::new(SocketVfsReadLease { lease : Some(lease) })),
            Err(SocketRecvError::Finished) => Ok(Box::new(EmptySocketReadLease)),
            Err(SocketRecvError::Busy | SocketRecvError::Empty) => {
                const O_NONBLOCK : usize = 0o4000;
                if self.socket
                       .status_flags() &
                   O_NONBLOCK !=
                   0
                {
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

    fn finish(self: Box<Self>, progress : VfsCopyProgress) -> VfsResult<VfsReadFinish> {
        if progress.copied != 0 {
            return Err(VfsError::Io);
        }
        Ok(VfsReadFinish::Bytes(0))
    }
}

struct SocketVfsReadLease {
    lease : Option<SocketReceiveLease>,
}

impl VfsReadLease for SocketVfsReadLease {
    fn bytes(&self) -> &[u8] {
        self.lease
            .as_ref()
            .map(SocketReceiveLease::bytes)
            .unwrap_or(&[])
    }

    fn finish(mut self: Box<Self>, progress : VfsCopyProgress) -> VfsResult<VfsReadFinish> {
        match self.lease
                  .take()
                  .ok_or(VfsError::Io)?
                  .finish(progress.copied, progress.complete)
                  .map_err(map_recv_stack_err)?
        {
            SocketRecvFinish::Bytes(copied) => Ok(VfsReadFinish::Bytes(copied)),
            SocketRecvFinish::Fault => Ok(VfsReadFinish::Fault),
        }
    }
}

/// TCP socket 的 VFS fd 句柄；覆盖创建、绑定、监听、连接等状态。
struct TcpSocketHandle {
    socket : SocketRef,
}

impl VfsIoHandle for TcpSocketHandle {
    fn resource_kind(&self) -> VfsResourceKind { VfsResourceKind::Socket }

    fn open_accmode(&self) -> u32 { 2 }

    fn open_status_flags(&self) -> u32 {
        self.socket
            .status_flags() as u32
    }

    fn set_open_status_flags(&mut self, flags : u32) -> VfsResult<()> {
        const O_NONBLOCK : usize = 0o4000;
        self.socket
            .set_status_flags(flags as usize & O_NONBLOCK);
        Ok(())
    }

    fn prepare_read(&mut self, max_len : usize) -> VfsResult<Box<dyn VfsPreparedRead>> {
        Ok(Box::new(SocketPreparedRead { socket : self.socket.clone(),
                                         max_len }))
    }

    fn read(&mut self, buf : &mut [u8]) -> VfsResult<usize> { read_with_lease(&self.socket, buf) }

    fn write(&mut self, buf : &[u8]) -> VfsResult<usize> {
        self.socket
            .send(buf)
            .map_err(map_send_stack_err)
    }

    fn poll_revents(&mut self, events : i16) -> VfsResult<i16> {
        let snapshot = self.socket
                           .poll_snapshot()
                           .map_err(map_stack_err)?;
        let mut revents = 0;
        if events & POLLIN != 0 {
            let read_ready = match snapshot.state {
                SocketState::Listening { .. } => snapshot.has_pending_accept,
                _ => snapshot.can_recv || !snapshot.may_recv,
            };
            if read_ready {
                revents |= POLLIN;
            }
        }
        if !matches!(snapshot.state,
                     SocketState::Listening { .. }) &&
           events & POLLOUT != 0 &&
           snapshot.may_send &&
           snapshot.send_capacity > 0
        {
            revents |= POLLOUT;
        }
        if snapshot.state == SocketState::Closed {
            revents |= POLLHUP;
        }
        Ok(revents)
    }

    fn close(&mut self) -> VfsResult<()> {
        // 底层 socket 由 SocketShared::drop 在最后一个 fd/在途操作释放时关闭。
        Ok(())
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> { Ok(socket_meta(self.socket.inode())) }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(Self { socket : self.socket.clone() }))
    }
}

/// UDP socket 的 fd 句柄。
struct UdpSocketHandle {
    socket : SocketRef,
}

impl VfsIoHandle for UdpSocketHandle {
    fn resource_kind(&self) -> VfsResourceKind { VfsResourceKind::Socket }

    fn open_accmode(&self) -> u32 { 2 }

    fn open_status_flags(&self) -> u32 {
        self.socket
            .status_flags() as u32
    }

    fn set_open_status_flags(&mut self, flags : u32) -> VfsResult<()> {
        const O_NONBLOCK : usize = 0o4000;
        self.socket
            .set_status_flags(flags as usize & O_NONBLOCK);
        Ok(())
    }

    fn prepare_read(&mut self, max_len : usize) -> VfsResult<Box<dyn VfsPreparedRead>> {
        Ok(Box::new(SocketPreparedRead { socket : self.socket.clone(),
                                         max_len }))
    }

    fn read(&mut self, buf : &mut [u8]) -> VfsResult<usize> { read_with_lease(&self.socket, buf) }

    fn write(&mut self, buf : &[u8]) -> VfsResult<usize> {
        self.socket
            .send(buf)
            .map_err(map_send_stack_err)
    }

    fn poll_revents(&mut self, events : i16) -> VfsResult<i16> {
        let snapshot = self.socket
                           .poll_snapshot()
                           .map_err(map_stack_err)?;
        let mut revents = 0;
        if events & POLLIN != 0 && snapshot.can_recv {
            revents |= POLLIN;
        }
        if events & POLLOUT != 0 && snapshot.may_send && snapshot.send_capacity > 0 {
            revents |= POLLOUT;
        }
        Ok(revents)
    }

    fn close(&mut self) -> VfsResult<()> { Ok(()) }

    fn metadata(&self) -> VfsResult<VfsMetadata> { Ok(socket_meta(self.socket.inode())) }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(Self { socket : self.socket.clone() }))
    }
}

fn read_with_lease(socket : &SocketRef, buf : &mut [u8]) -> VfsResult<usize> {
    let lease = socket.prepare_receive(buf.len())
                      .map_err(map_recv_stack_err)?;
    let len = lease.bytes().len();
    buf[..len].copy_from_slice(lease.bytes());
    match lease.finish(len, true)
               .map_err(map_recv_stack_err)?
    {
        SocketRecvFinish::Bytes(copied) => Ok(copied),
        SocketRecvFinish::Fault => Err(VfsError::Io),
    }
}

/// 将协议栈语义错误映射为 VFS 错误码。
fn map_stack_err(err : NetworkError) -> VfsError {
    match err {
        NetworkError::InvalidSocket |
        NetworkError::WrongSocketType |
        NetworkError::NotConnected => VfsError::BadFd,
        NetworkError::Io | NetworkError::Internal | NetworkError::StackUnavailable => VfsError::Io,
        _ => VfsError::Unsupported,
    }
}

fn map_send_stack_err(err : SocketSendError) -> VfsError {
    match err {
        SocketSendError::WouldBlock => VfsError::WouldBlock,
        SocketSendError::InvalidSocket | SocketSendError::NotConnected => VfsError::BadFd,
        SocketSendError::MessageTooLarge |
        SocketSendError::NoBufferSpace |
        SocketSendError::InvalidDestination |
        SocketSendError::StackUnavailable |
        SocketSendError::Io => VfsError::Io,
    }
}

fn map_recv_stack_err(err : SocketRecvError) -> VfsError {
    match err {
        SocketRecvError::Busy => VfsError::Busy,
        SocketRecvError::Empty => VfsError::WouldBlock,
        SocketRecvError::Finished => VfsError::Unsupported,
        SocketRecvError::InvalidSocket => VfsError::BadFd,
        SocketRecvError::NoMemory => VfsError::NoMemory,
        SocketRecvError::Io => VfsError::Io,
    }
}

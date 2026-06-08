//! Socket 文件描述符句柄：将 smoltcp [`SocketHandle`] 桥接到 VFS [`VfsIoHandle`] trait。
//!
//! 仅在 `impl-smoltcp` feature 启用时编译。

use alloc::boxed::Box;
use alloc::sync::Arc;
use smoltcp::iface::SocketHandle;
use spin::Mutex;
use vfs_api::error::{VfsError, VfsResult};
use vfs_api::handle::VfsIoHandle;
use vfs_api::meta::{VfsMetadata, VfsNodeType};

use crate::stack;

fn socket_meta() -> VfsMetadata {
    VfsMetadata {
        node_type: VfsNodeType::Special,
        size: 0,
        mode: 0o140777, // srwxrwxrwx
    }
}

/// socket fd 共享状态：fd handle 与 syscall 映射表共同持有它。
#[derive(Clone)]
pub struct SocketRef {
    inner: Arc<Mutex<SocketHandle>>,
}

impl SocketRef {
    pub fn new(handle: SocketHandle) -> Self {
        Self {
            inner: Arc::new(Mutex::new(handle)),
        }
    }

    pub fn handle(&self) -> SocketHandle {
        *self.inner.lock()
    }

    pub fn replace_handle(&self, handle: SocketHandle) {
        *self.inner.lock() = handle;
    }

    fn should_close_underlying(&self) -> bool {
        Arc::strong_count(&self.inner) <= 2
    }
}

/// TCP 已连接 socket 的 fd 句柄。
pub struct TcpStreamHandle {
    pub socket: SocketRef,
}

impl VfsIoHandle for TcpStreamHandle {
    fn read(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        stack::socket_recv(self.socket.handle(), buf).map_err(map_stack_err)
    }

    fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        stack::socket_send(self.socket.handle(), buf).map_err(map_stack_err)
    }

    fn close(&mut self) -> VfsResult<()> {
        if self.socket.should_close_underlying() {
            stack::socket_close(self.socket.handle()).map_err(map_stack_err)
        } else {
            Ok(())
        }
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(socket_meta())
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
    fn close(&mut self) -> VfsResult<()> {
        if self.socket.should_close_underlying() {
            stack::socket_close(self.socket.handle()).map_err(map_stack_err)
        } else {
            Ok(())
        }
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(socket_meta())
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
    fn read(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        // UDP read: recvfrom 并丢弃来源地址
        stack::socket_recvfrom(self.socket.handle(), buf)
            .map(|(n, _ip, _port)| n)
            .map_err(map_stack_err)
    }

    fn write(&mut self, _buf: &[u8]) -> VfsResult<usize> {
        // UDP write without destination → error if not connected
        Err(VfsError::Unsupported)
    }

    fn close(&mut self) -> VfsResult<()> {
        if self.socket.should_close_underlying() {
            stack::socket_close(self.socket.handle()).map_err(map_stack_err)
        } else {
            Ok(())
        }
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(socket_meta())
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(Self {
            socket: self.socket.clone(),
        }))
    }
}

fn map_stack_err(err: &'static str) -> VfsError {
    match err {
        "no connected tcp socket" | "invalid socket handle" | "not a tcp socket" => VfsError::BadFd,
        "recv failed" | "send failed" | "udp recvfrom failed" => VfsError::Io,
        _ => VfsError::Unsupported,
    }
}

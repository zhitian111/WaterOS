//! Socket 文件描述符句柄：将 smoltcp [`SocketHandle`] 桥接到 VFS [`VfsIoHandle`] trait。
//!
//! 仅在 `impl-smoltcp` feature 启用时编译。

use smoltcp::iface::SocketHandle;
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

/// TCP 已连接 socket 的 fd 句柄。
pub struct TcpStreamHandle {
    pub handle: SocketHandle,
}

impl VfsIoHandle for TcpStreamHandle {
    fn read(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        stack::socket_recv(self.handle, buf).map_err(map_stack_err)
    }

    fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        stack::socket_send(self.handle, buf).map_err(map_stack_err)
    }

    fn close(&mut self) -> VfsResult<()> {
        stack::socket_close(self.handle).map_err(map_stack_err)
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(socket_meta())
    }
}

/// TCP 监听 socket 的 fd 句柄。
pub struct TcpListenerHandle {
    pub handle: SocketHandle,
}

impl VfsIoHandle for TcpListenerHandle {
    fn close(&mut self) -> VfsResult<()> {
        stack::socket_close(self.handle).map_err(map_stack_err)
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(socket_meta())
    }
}

/// UDP socket 的 fd 句柄。
pub struct UdpSocketHandle {
    pub handle: SocketHandle,
}

impl VfsIoHandle for UdpSocketHandle {
    fn read(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        // UDP read: recvfrom 并丢弃来源地址
        stack::socket_recvfrom(self.handle, buf)
            .map(|(n, _ip, _port)| n)
            .map_err(map_stack_err)
    }

    fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        // UDP write without destination → error if not connected
        Err(VfsError::Unsupported)
    }

    fn close(&mut self) -> VfsResult<()> {
        stack::socket_close(self.handle).map_err(map_stack_err)
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(socket_meta())
    }
}

fn map_stack_err(err: &'static str) -> VfsError {
    match err {
        "no connected tcp socket" | "invalid socket handle" | "not a tcp socket" => VfsError::BadFd,
        "recv failed" | "send failed" | "udp recvfrom failed" => VfsError::Io,
        _ => VfsError::Unsupported,
    }
}

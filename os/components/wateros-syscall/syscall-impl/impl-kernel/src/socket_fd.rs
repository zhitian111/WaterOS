//! 从统一 VFS fd 表识别 inet socket 句柄。
//!
//! socket 不再维护第二张 fd 映射表；`dup`、`close`、`fork/clone` 的生命周期
//! 统一由 VFS fd 表管理，避免多核下两张表分步更新产生不一致。

use driver_network::{SocketRef, TcpListenerHandle, TcpStreamHandle, UdpSocketHandle};
use vfs::VfsIoHandle;

fn socket_ref(handle: &(dyn VfsIoHandle + '_)) -> Option<SocketRef> {
    if let Some(handle) = handle.as_any().downcast_ref::<TcpStreamHandle>() {
        return Some(handle.socket.clone());
    }
    if let Some(handle) = handle.as_any().downcast_ref::<TcpListenerHandle>() {
        return Some(handle.socket.clone());
    }
    handle
        .as_any()
        .downcast_ref::<UdpSocketHandle>()
        .map(|handle| handle.socket.clone())
}

pub(crate) fn lookup(fd: usize) -> Option<SocketRef> {
    vfs::fd::with_current_io(fd, |handle| Ok(socket_ref(handle)))
        .ok()
        .flatten()
}

/// 查找 inet socket fd；无效 fd 返回 `EBADF`，有效非 socket 返回 `ENOTSOCK`。
pub(crate) fn lookup_or_errno(fd: usize) -> Result<SocketRef, abi::errno::ErrNo> {
    match vfs::fd::with_current_io(fd, |handle| Ok(socket_ref(handle))) {
        Ok(Some(socket)) => Ok(socket),
        Ok(None) => Err(abi::errno::ErrNo::ENOTSOCK),
        Err(_) => Err(abi::errno::ErrNo::EBADF),
    }
}

pub(crate) fn status_flags(fd: usize) -> Option<usize> {
    lookup(fd).map(|socket| socket.status_flags())
}

pub(crate) fn set_status_flags(fd: usize, flags: usize) -> Option<()> {
    let socket = lookup(fd)?;
    socket.set_status_flags(flags);
    Some(())
}

pub(crate) fn is_nonblocking(fd: usize) -> bool {
    const O_NONBLOCK: usize = 0o0004000;
    status_flags(fd).is_some_and(|flags| flags & O_NONBLOCK != 0)
}

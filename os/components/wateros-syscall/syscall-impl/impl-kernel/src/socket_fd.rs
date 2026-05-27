//! Socket fd → smoltcp [`SocketHandle`] 映射表。
//!
//! 因 [`VfsIoHandle`] 不支持向下转型，每个 socket fd 的 smoltcp 句柄在此独立维护。

use alloc::collections::BTreeMap;
use smoltcp::iface::SocketHandle;
use spin::Mutex;

static SOCKET_FD_MAP: Mutex<BTreeMap<usize, SocketHandle>> = Mutex::new(BTreeMap::new());

pub(crate) fn register(fd: usize, handle: SocketHandle) {
    SOCKET_FD_MAP.lock().insert(fd, handle);
}

pub(crate) fn lookup(fd: usize) -> Option<SocketHandle> {
    SOCKET_FD_MAP.lock().get(&fd).copied()
}

pub(crate) fn remove(fd: usize) {
    SOCKET_FD_MAP.lock().remove(&fd);
}

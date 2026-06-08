//! Socket fd → network [`SocketRef`] 映射表。
//!
//! 因 [`VfsIoHandle`] 不支持向下转型，每个 socket fd 的共享 socket 状态在此独立维护。

use alloc::collections::BTreeMap;
use driver_network::SocketRef;
use spin::Mutex;

static SOCKET_FD_MAP: Mutex<BTreeMap<usize, SocketRef>> = Mutex::new(BTreeMap::new());

pub(crate) fn register(fd: usize, socket: SocketRef) {
    SOCKET_FD_MAP
        .lock()
        .insert(fd, socket);
}

pub(crate) fn lookup(fd: usize) -> Option<SocketRef> {
    SOCKET_FD_MAP
        .lock()
        .get(&fd)
        .cloned()
}

pub(crate) fn remove(fd: usize) {
    SOCKET_FD_MAP
        .lock()
        .remove(&fd);
}

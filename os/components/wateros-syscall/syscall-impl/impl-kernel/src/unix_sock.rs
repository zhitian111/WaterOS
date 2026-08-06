//! AF_UNIX 域套接字：pathname / abstract bind、stream listen/accept/connect、dgram 投递。

//! 本模块代码由AI完成
extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use api_v0::ErrNo;
use spin::Mutex;
use vfs::api::resolve_open_path;
use vfs::api::handle::VfsIoHandle;
use vfs::api::{
    SingleRootReadView, VfsCopyProgress, VfsError, VfsMetadata, VfsNodeType, VfsPreparedRead,
    VfsReadFinish, VfsReadLease, VfsResult,
};
use vfs::UnixStreamPairEnd;

use crate::socket_block::socket_blocking_tick;
use crate::user_copy::{
    copy_from_user, copy_to_user, copy_to_user_progress, copy_to_user_struct,
};
use crate::vfs_util::vfs_error_to_errno;

const AF_UNIX: u16 = 1;
const SOCK_STREAM: usize = 1;
const SOCK_DGRAM: usize = 2;
const SOCK_SEQPACKET: usize = 5;
const SOCK_NONBLOCK: usize = 0o4000;

/// 单 listen socket 待 accept 连接队列上限。
const UNIX_ACCEPT_QUEUE_MAX : usize = 128;

/// 单 dgram bind 表项收件队列上限。
const UNIX_DGRAM_INBOX_MAX : usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
// 本结构代码由AI完成
pub(crate) enum UnixSockType {
    Stream,
    Dgram,
}

#[derive(Clone)]
pub(crate) struct UnixSockRef {
    inner: Arc<Mutex<UnixSockInner>>,
}

struct UnixSockInner {
    sock_type: UnixSockType,
    nonblocking: bool,
    bound_key: Option<Vec<u8>>,
    peer_key: Option<Vec<u8>>,
    listening: bool,
    endpoint: Option<UnixStreamPairEnd>,
    dgram_peer: Option<Vec<u8>>,
    dgram_peer_inbox: Option<Arc<DgramInbox>>,
    dgram_inbox: Option<Arc<DgramInbox>>,
}

struct BoundEntry {
    sock_type: UnixSockType,
    listening: bool,
    accept_queue: VecDeque<UnixStreamPairEnd>,
    dgram_inbox: Option<Arc<DgramInbox>>,
}

struct DgramPacket {
    data: Vec<u8>,
    sender: Option<Vec<u8>>,
}

struct DgramInboxState {
    queue: VecDeque<DgramPacket>,
    active_reservation: Option<u64>,
    next_reservation_id: u64,
    closed: bool,
}

struct DgramInbox {
    state: Mutex<DgramInboxState>,
    read_wait: task::WaitQueue,
}

impl DgramInbox {
    fn new() -> Self {
        Self {
            state: Mutex::new(DgramInboxState {
                queue: VecDeque::new(),
                active_reservation: None,
                next_reservation_id: 1,
                closed: false,
            }),
            read_wait: task::WaitQueue::new_named("unix-dgram-read"),
        }
    }

    fn push(&self, packet: DgramPacket) -> Result<(), ErrNo> {
        let mut state = self.state.lock();
        if state.closed {
            return Err(ErrNo::ECONNREFUSED);
        }
        let occupied = state.queue.len() + usize::from(state.active_reservation.is_some());
        if occupied >= UNIX_DGRAM_INBOX_MAX {
            return Err(ErrNo::EAGAIN);
        }
        state.queue.push_back(packet);
        drop(state);
        self.read_wait.wake_all();
        Ok(())
    }

    fn acquire(self: &Arc<Self>, nonblocking: bool) -> Result<DgramReadLease, VfsError> {
        loop {
            let mut state = self.state.lock();
            if state.closed {
                return Err(VfsError::BadFd);
            }
            if state.active_reservation.is_none() {
                if let Some(packet) = state.queue.pop_front() {
                    let id = state.next_reservation_id;
                    state.next_reservation_id = state.next_reservation_id.wrapping_add(1).max(1);
                    state.active_reservation = Some(id);
                    drop(state);
                    return Ok(DgramReadLease {
                        inbox: self.clone(),
                        reservation_id: Some(id),
                        packet: Some(packet),
                    });
                }
            }
            if nonblocking {
                return Err(VfsError::WouldBlock);
            }
            drop(state);
            let result = self.read_wait.wait_current_while(|| {
                let state = self.state.lock();
                !state.closed &&
                (state.active_reservation.is_some() || state.queue.is_empty())
            });
            if result == task::TaskWaitResult::Interrupted {
                return Err(VfsError::Interrupted);
            }
        }
    }

    fn has_data(&self) -> bool {
        let state = self.state.lock();
        state.active_reservation.is_none() && !state.queue.is_empty()
    }

    fn close(&self) {
        self.state.lock().closed = true;
        self.read_wait.wake_all();
    }
}

static NEXT_INODE: AtomicU64 = AtomicU64::new(0x4_0000);
static FD_TABLE: Mutex<BTreeMap<(usize, usize), UnixSockRef>> = Mutex::new(BTreeMap::new());
static BOUND: Mutex<BTreeMap<Vec<u8>, BoundEntry>> = Mutex::new(BTreeMap::new());

pub(crate) struct UnixSocketHandle {
    sock: UnixSockRef,
    inode: u64,
}

struct DgramReadLease {
    inbox: Arc<DgramInbox>,
    reservation_id: Option<u64>,
    packet: Option<DgramPacket>,
}

impl DgramReadLease {
    fn bytes(&self, max_len: usize) -> &[u8] {
        let data = self
            .packet
            .as_ref()
            .map(|packet| packet.data.as_slice())
            .unwrap_or(&[]);
        &data[..data.len().min(max_len)]
    }

    fn sender(&self) -> Option<&[u8]> {
        self.packet
            .as_ref()
            .and_then(|packet| packet.sender.as_deref())
    }

    fn finish(
        mut self,
        copied: usize,
        complete: bool,
    ) -> VfsResult<VfsReadFinish> {
        let id = self.reservation_id.take().ok_or(VfsError::Io)?;
        let packet = self.packet.take().ok_or(VfsError::Io)?;
        let mut state = self.inbox.state.lock();
        if state.active_reservation != Some(id) {
            return Err(VfsError::Io);
        }
        state.active_reservation = None;
        let finish = if complete {
            VfsReadFinish::Bytes(copied)
        } else if copied == 0 {
            state.queue.push_front(packet);
            VfsReadFinish::Fault
        } else {
            VfsReadFinish::Fault
        };
        drop(state);
        self.inbox.read_wait.wake_all();
        Ok(finish)
    }
}

impl Drop for DgramReadLease {
    fn drop(&mut self) {
        let (Some(id), Some(packet)) =
            (self.reservation_id.take(), self.packet.take())
        else {
            return;
        };
        let mut state = self.inbox.state.lock();
        if state.active_reservation == Some(id) {
            state.active_reservation = None;
            state.queue.push_front(packet);
        }
        drop(state);
        self.inbox.read_wait.wake_all();
    }
}

pub(crate) fn is_unix_fd(fd: usize) -> bool {
    let task_id = match vfs::fd::current_task_id() {
        Ok(id) => id,
        Err(_) => return false,
    };
    FD_TABLE.lock().contains_key(&(task_id, fd))
}

pub(crate) fn register(fd: usize, sock: UnixSockRef) {
    let task_id = vfs::fd::current_task_id().expect("unix register requires task");
    FD_TABLE.lock().insert((task_id, fd), sock);
}

pub(crate) fn unregister(task_id: usize, fd: usize) {
    let mut table = FD_TABLE.lock();
    let Some(sock) = table.remove(&(task_id, fd)) else {
        return;
    };
    let still_referenced = table
        .values()
        .any(|other| Arc::ptr_eq(&other.inner, &sock.inner));
    drop(table);
    cleanup_removed_socket(sock, still_referenced);
}

pub(crate) fn duplicate_registration(task_id: usize, oldfd: usize, newfd: usize) {
    let mut table = FD_TABLE.lock();
    let source = table.get(&(task_id, oldfd)).cloned();
    let displaced = table.remove(&(task_id, newfd));
    if let Some(source) = source {
        table.insert((task_id, newfd), source);
    }
    let displaced_still_referenced = displaced.as_ref().is_some_and(|sock| {
        table
            .values()
            .any(|other| Arc::ptr_eq(&other.inner, &sock.inner))
    });
    drop(table);
    if let Some(displaced) = displaced {
        cleanup_removed_socket(displaced, displaced_still_referenced);
    }
}

fn cleanup_removed_socket(sock: UnixSockRef, still_referenced: bool) {
    let (key, inbox) = {
        let inner = sock.inner.lock();
        (inner.bound_key.clone(), inner.dgram_inbox.clone())
    };
    drop(sock);
    if !still_referenced {
        if let Some(inbox) = inbox {
            inbox.close();
        }
        if let Some(key) = key {
            BOUND.lock().remove(&key);
        }
    }
}

pub(crate) fn copy_fds_from_parent(child: usize, parent: usize) {
    let mut table = FD_TABLE.lock();
    let inherited: Vec<_> = table
        .range((parent, 0)..=(parent, usize::MAX))
        .map(|(&(_, fd), sock)| {
            ((child, fd), sock.clone())
        })
        .collect();
    for (key, sock) in inherited {
        table.insert(key, sock);
    }
}

pub(crate) fn drop_task(task_id: usize) {
    let fds: Vec<usize> = FD_TABLE
        .lock()
        .range((task_id, 0)..=(task_id, usize::MAX))
        .map(|(&(_, fd), _)| fd)
        .collect();
    for fd in fds {
        unregister(task_id, fd);
    }
}

// 本方法代码由AI完成
#[allow(private_interfaces)]
pub(crate) fn alloc_unix_socket(
    typ: usize,
    status_flags: usize,
) -> Result<(Box<dyn VfsIoHandle>, UnixSockRef), ErrNo> {
    let sock_type = match typ {
        SOCK_STREAM | SOCK_SEQPACKET => UnixSockType::Stream,
        SOCK_DGRAM => UnixSockType::Dgram,
        _ => return Err(ErrNo::EPROTONOSUPPORT),
    };
    let nonblocking = status_flags & SOCK_NONBLOCK != 0;
    let dgram_inbox =
        (sock_type == UnixSockType::Dgram).then(|| Arc::new(DgramInbox::new()));
    let sock = UnixSockRef {
        inner: Arc::new(Mutex::new(UnixSockInner {
            sock_type,
            nonblocking,
            bound_key: None,
            peer_key: None,
            listening: false,
            endpoint: None,
            dgram_peer: None,
            dgram_peer_inbox: None,
            dgram_inbox,
        })),
    };
    let inode = NEXT_INODE.fetch_add(1, Ordering::Relaxed);
    let handle = Box::new(UnixSocketHandle { sock: sock.clone(), inode });
    Ok((handle, sock))
}

/// Create the two registered AF_UNIX endpoints used by `socketpair(2)`.
#[allow(private_interfaces)]
pub(crate) fn alloc_unix_stream_pair(
    nonblocking: bool,
) -> (
    (Box<dyn VfsIoHandle>, UnixSockRef),
    (Box<dyn VfsIoHandle>, UnixSockRef),
) {
    let (endpoint0, endpoint1) = vfs::stream_pair_handle_pair(nonblocking);
    let make_socket = |endpoint| {
        let sock = UnixSockRef {
            inner: Arc::new(Mutex::new(UnixSockInner {
                sock_type: UnixSockType::Stream,
                nonblocking,
                bound_key: None,
                peer_key: Some(Vec::new()),
                listening: false,
                endpoint: Some(endpoint),
                dgram_peer: None,
                dgram_peer_inbox: None,
                dgram_inbox: None,
            })),
        };
        let inode = NEXT_INODE.fetch_add(1, Ordering::Relaxed);
        let handle: Box<dyn VfsIoHandle> =
            Box::new(UnixSocketHandle { sock: sock.clone(), inode });
        (handle, sock)
    };
    (make_socket(endpoint0), make_socket(endpoint1))
}

/// Create a connected AF_UNIX datagram pair with one inbox per endpoint.
#[allow(private_interfaces)]
pub(crate) fn alloc_unix_dgram_pair(
    nonblocking: bool,
) -> (
    (Box<dyn VfsIoHandle>, UnixSockRef),
    (Box<dyn VfsIoHandle>, UnixSockRef),
) {
    let inbox0 = Arc::new(DgramInbox::new());
    let inbox1 = Arc::new(DgramInbox::new());
    let make_socket = |inbox: Arc<DgramInbox>, peer: Arc<DgramInbox>| {
        let sock = UnixSockRef {
            inner: Arc::new(Mutex::new(UnixSockInner {
                sock_type: UnixSockType::Dgram,
                nonblocking,
                bound_key: None,
                peer_key: Some(Vec::new()),
                listening: false,
                endpoint: None,
                dgram_peer: None,
                dgram_peer_inbox: Some(peer),
                dgram_inbox: Some(inbox),
            })),
        };
        let inode = NEXT_INODE.fetch_add(1, Ordering::Relaxed);
        let handle: Box<dyn VfsIoHandle> =
            Box::new(UnixSocketHandle { sock: sock.clone(), inode });
        (handle, sock)
    };
    (make_socket(inbox0.clone(), inbox1.clone()), make_socket(inbox1, inbox0))
}

#[allow(private_interfaces)]
pub(crate) fn parse_sockaddr_un(addr_ptr: usize, addrlen: usize) -> Result<UnixAddr, ErrNo> {
    if addrlen < 2 || addr_ptr == 0 {
        return Err(ErrNo::EINVAL);
    }
    let mut hdr = [0u8; 2];
    copy_from_user(&mut hdr, addr_ptr)?;
    let family = u16::from_ne_bytes(hdr);
    if family != AF_UNIX {
        return Err(ErrNo::EINVAL);
    }
    let path_len = addrlen.saturating_sub(2);
    if path_len == 0 {
        return Ok(UnixAddr { key: Vec::new(), abstract_ns: false });
    }
    let mut raw = vec![0u8; path_len];
    copy_from_user(&mut raw, addr_ptr + 2)?;
    let abstract_ns = raw.first() == Some(&0);
    let key = if abstract_ns {
        raw
    } else {
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        let rel = core::str::from_utf8(&raw[..end]).map_err(|_| ErrNo::EINVAL)?;
        let abs = resolve_open_path(rel).map_err(vfs_error_to_errno)?;
        abs.into_bytes()
    };
    Ok(UnixAddr { key, abstract_ns })
}

struct UnixAddr {
    key: Vec<u8>,
    abstract_ns: bool,
}

// 本方法代码由AI完成
pub(crate) fn bind(fd: usize, addr_ptr: usize, addrlen: usize) -> Result<(), ErrNo> {
    let addr = parse_sockaddr_un(addr_ptr, addrlen)?;
    let sock = lookup_current(fd)?;
    {
        let inner = sock.inner.lock();
        if inner.bound_key.is_some() {
            return Err(ErrNo::EINVAL);
        }
    }
    if !addr.abstract_ns {
        validate_pathname_bind(&addr.key)?;
        if !addr.key.is_empty() {
            install_pathname_socket(&addr.key)?;
        }
    }
    let mut inner = sock.inner.lock();
    let mut bound = BOUND.lock();
    if bound.contains_key(&addr.key) {
        return Err(ErrNo::EADDRINUSE);
    }
    let dgram_inbox = inner.dgram_inbox.clone();
    bound.insert(
        addr.key.clone(),
        BoundEntry {
            sock_type: inner.sock_type,
            listening: false,
            accept_queue: VecDeque::new(),
            dgram_inbox,
        },
    );
    inner.bound_key = Some(addr.key);
    Ok(())
}

// 本方法代码由AI完成
pub(crate) fn listen(fd: usize, _backlog: usize) -> Result<(), ErrNo> {
    let sock = lookup_current(fd)?;
    let mut inner = sock.inner.lock();
    let key = inner.bound_key.clone().ok_or(ErrNo::EINVAL)?;
    if inner.sock_type != UnixSockType::Stream {
        return Err(ErrNo::EOPNOTSUPP);
    }
    inner.listening = true;
    let mut bound = BOUND.lock();
    let entry = bound.get_mut(&key).ok_or(ErrNo::EINVAL)?;
    entry.listening = true;
    Ok(())
}

// 本方法代码由AI完成
pub(crate) fn connect(fd: usize, addr_ptr: usize, addrlen: usize) -> Result<(), ErrNo> {
    let addr = parse_sockaddr_un(addr_ptr, addrlen)?;
    let sock = lookup_current(fd)?;
    let mut inner = sock.inner.lock();
    match inner.sock_type {
        UnixSockType::Stream => connect_stream(&mut inner, &addr.key),
        UnixSockType::Dgram => {
            inner.dgram_peer = Some(addr.key);
            inner.dgram_peer_inbox = None;
            Ok(())
        }
    }
}

fn connect_stream(inner: &mut UnixSockInner, key: &[u8]) -> Result<(), ErrNo> {
    if inner.endpoint.is_some() {
        return Err(ErrNo::EINVAL);
    }
    let mut bound = BOUND.lock();
    let entry = bound.get_mut(key).ok_or(ErrNo::ECONNREFUSED)?;
    if entry.sock_type != UnixSockType::Stream || !entry.listening {
        return Err(ErrNo::ECONNREFUSED);
    }
    let (client_end, server_end) = vfs::stream_pair_handle_pair(inner.nonblocking);
    if entry.accept_queue.len() >= UNIX_ACCEPT_QUEUE_MAX {
        log::warn!("[unix_sock] accept_queue full key_len={} cap={}",
                   key.len(),
                   UNIX_ACCEPT_QUEUE_MAX);
        return Err(ErrNo::EAGAIN);
    }
    entry.accept_queue.push_back(server_end);
    inner.endpoint = Some(client_end);
    inner.peer_key = Some(key.to_vec());
    Ok(())
}

pub(crate) fn getsockname(
    fd: usize,
    addr_ptr: usize,
    addrlen_ptr: usize,
) -> Result<(), ErrNo> {
    let sock = lookup_current(fd)?;
    let key = sock
        .inner
        .lock()
        .bound_key
        .clone()
        .ok_or(ErrNo::EINVAL)?;
    write_unix_addr_to_user(addr_ptr, addrlen_ptr, &key)
}

pub(crate) fn getpeername(
    fd: usize,
    addr_ptr: usize,
    addrlen_ptr: usize,
) -> Result<(), ErrNo> {
    let sock = lookup_current(fd)?;
    let inner = sock.inner.lock();
    let key = inner
        .peer_key
        .clone()
        .or_else(|| inner.dgram_peer.clone())
        .ok_or(ErrNo::ENOTCONN)?;
    write_unix_addr_to_user(addr_ptr, addrlen_ptr, &key)
}

pub(crate) fn accept(fd: usize) -> Result<(Box<dyn VfsIoHandle>, UnixSockRef), ErrNo> {
    let sock = lookup_current(fd)?;
    let key = {
        let inner = sock.inner.lock();
        if inner.sock_type != UnixSockType::Stream || !inner.listening {
            return Err(ErrNo::EINVAL);
        }
        inner.bound_key.clone().ok_or(ErrNo::EINVAL)?
    };
    loop {
        let server_end = {
            let mut bound = BOUND.lock();
            let entry = bound.get_mut(&key).ok_or(ErrNo::EINVAL)?;
            entry.accept_queue.pop_front()
        };
        if let Some(end) = server_end {
            let accepted = UnixSockRef {
                inner: Arc::new(Mutex::new(UnixSockInner {
                    sock_type: UnixSockType::Stream,
                    nonblocking: sock.inner.lock().nonblocking,
                    bound_key: None,
                    peer_key: None,
                    listening: false,
                    endpoint: Some(end),
                    dgram_peer: None,
                    dgram_peer_inbox: None,
                    dgram_inbox: None,
                })),
            };
            let inode = NEXT_INODE.fetch_add(1, Ordering::Relaxed);
            return Ok((Box::new(UnixSocketHandle { sock: accepted.clone(), inode }), accepted));
        }
        let nonblocking = sock.inner.lock().nonblocking;
        if nonblocking {
            return Err(ErrNo::EAGAIN);
        }
        let task_id = task::current_task_id().unwrap_or(0);
        socket_blocking_tick(false, task_id)?;
    }
}

pub(crate) fn sendto_unix(
    fd: usize,
    buf: &[u8],
    addr_ptr: usize,
    addrlen: usize,
) -> Result<usize, ErrNo> {
    let sock = lookup_current(fd)?;
    let inner = sock.inner.lock();
    if let Some(mut endpoint) = inner.endpoint.clone() {
        drop(inner);
        if addr_ptr != 0 && addrlen != 0 {
            return Err(ErrNo::EINVAL);
        }
        return endpoint.write(buf).map_err(vfs_error_to_errno);
    }
    let sender_key = inner.bound_key.clone();
    if addr_ptr == 0 && addrlen == 0 {
        if let Some(peer) = inner.dgram_peer_inbox.clone() {
            drop(inner);
            peer.push(DgramPacket { data: buf.to_vec(),
                                    sender: sender_key })?;
            return Ok(buf.len());
        }
    }
    let key = if addr_ptr != 0 && addrlen >= 2 {
        parse_sockaddr_un(addr_ptr, addrlen)?.key
    } else {
        inner.dgram_peer.clone().ok_or(ErrNo::ENOTCONN)?
    };
    if inner.sock_type != UnixSockType::Dgram {
        return Err(ErrNo::EOPNOTSUPP);
    }
    drop(inner);
    let bound = BOUND.lock();
    let entry = bound.get(&key).ok_or(ErrNo::ECONNREFUSED)?;
    if entry.sock_type != UnixSockType::Dgram {
        return Err(ErrNo::ECONNREFUSED);
    }
    let inbox = entry.dgram_inbox.clone().ok_or(ErrNo::ECONNREFUSED)?;
    drop(bound);
    inbox.push(DgramPacket {
        data: buf.to_vec(),
        sender: sender_key,
    })?;
    Ok(buf.len())
}

pub(crate) fn recvfrom_unix(
    fd: usize,
    buf_ptr: usize,
    max_len: usize,
    addr_ptr: usize,
    addrlen_ptr: usize,
) -> Result<usize, ErrNo> {
    let sock = lookup_current(fd)?;
    let (endpoint, inbox, nonblocking) = {
        let inner = sock.inner.lock();
        (inner.endpoint.clone(), inner.dgram_inbox.clone(), inner.nonblocking)
    };
    if endpoint.is_some() {
        let prepared =
            vfs::fd::prepare_current_read(fd, max_len).map_err(vfs_error_to_errno)?;
        let lease = prepared.acquire().map_err(vfs_error_to_errno)?;
        let progress = copy_to_user_progress(buf_ptr, lease.bytes());
        let finish = lease
            .finish(VfsCopyProgress {
                copied: progress.copied,
                complete: progress.error.is_none(),
            })
            .map_err(vfs_error_to_errno)?;
        return match finish {
            VfsReadFinish::Bytes(copied) => Ok(copied),
            VfsReadFinish::Fault => Err(progress.error.unwrap_or(ErrNo::EFAULT)),
        };
    }
    let inbox = inbox.ok_or(ErrNo::ENOTCONN)?;
    let lease = inbox.acquire(nonblocking).map_err(vfs_error_to_errno)?;
    let sender = lease.sender().map(Vec::from);
    let progress = copy_to_user_progress(buf_ptr, lease.bytes(max_len));
    let finish = lease
        .finish(progress.copied, progress.error.is_none())
        .map_err(vfs_error_to_errno)?;
    match finish {
        VfsReadFinish::Fault => Err(progress.error.unwrap_or(ErrNo::EFAULT)),
        VfsReadFinish::Bytes(copied) => {
            if addr_ptr != 0 && addrlen_ptr != 0 {
                if let Some(sender_key) = sender {
                    write_unix_addr_to_user(addr_ptr, addrlen_ptr, &sender_key)?;
                }
            }
            Ok(copied)
        }
    }
}

fn write_unix_addr_to_user(
    addr_ptr: usize,
    addrlen_ptr: usize,
    key: &[u8],
) -> Result<(), ErrNo> {
    use crate::user_copy::copy_from_user_struct;
    let mut addr = vec![0u8; 2 + key.len()];
    addr[0..2].copy_from_slice(&AF_UNIX.to_ne_bytes());
    if key.first() == Some(&0) {
        addr[2..2 + key.len()].copy_from_slice(key);
    } else {
        let path = core::str::from_utf8(key).map_err(|_| ErrNo::EINVAL)?;
        let bytes = path.as_bytes();
        let copy_len = bytes.len().min(addr.len() - 2);
        addr[2..2 + copy_len].copy_from_slice(&bytes[..copy_len]);
    }
    let addrlen = copy_from_user_struct::<u32>(addrlen_ptr)?;
    let write_len = addr.len().min(addrlen as usize);
    copy_to_user(addr_ptr, &addr[..write_len])?;
    copy_to_user_struct(addrlen_ptr, &(write_len as u32))?;
    Ok(())
}

fn deliver_dgram(peer: &[u8], buf: &[u8], sender: Option<Vec<u8>>) -> Result<usize, ErrNo> {
    let bound = BOUND.lock();
    let entry = bound.get(peer).ok_or(ErrNo::ECONNREFUSED)?;
    if entry.sock_type != UnixSockType::Dgram {
        return Err(ErrNo::ECONNREFUSED);
    }
    let inbox = entry.dgram_inbox.clone().ok_or(ErrNo::ECONNREFUSED)?;
    drop(bound);
    inbox.push(DgramPacket {
        data: buf.to_vec(),
        sender,
    })?;
    Ok(buf.len())
}

fn lookup_current(fd: usize) -> Result<UnixSockRef, ErrNo> {
    let task_id = vfs::fd::current_task_id().map_err(vfs_error_to_errno)?;
    if let Some(sock) = FD_TABLE.lock().get(&(task_id, fd)).cloned() {
        return Ok(sock);
    }
    if vfs::fd::with_current_io(fd, |_| Ok(())).is_ok() {
        Err(ErrNo::ENOTSOCK)
    } else {
        Err(ErrNo::EBADF)
    }
}

fn validate_pathname_bind(key: &[u8]) -> Result<(), ErrNo> {
    let path = core::str::from_utf8(key).map_err(|_| ErrNo::EINVAL)?;
    if path.is_empty() {
        return Ok(());
    }
    let parent = path.rsplit_once('/').map(|(p, _)| p).unwrap_or("/");
    let parent = if parent.is_empty() { "/" } else { parent };
    let backend = vfs::active_impl::backend();
    match backend.metadata(parent) {
        Ok(meta) if meta.node_type == VfsNodeType::Directory => Ok(()),
        Ok(_) => Err(ErrNo::ENOTDIR),
        Err(VfsError::NotFound) => Err(ErrNo::ENOENT),
        Err(_) => Err(ErrNo::ENOTDIR),
    }
}

fn install_pathname_socket(key: &[u8]) -> Result<(), ErrNo> {
    let path = core::str::from_utf8(key).map_err(|_| ErrNo::EINVAL)?;
    let backend = vfs::active_impl::backend();
    if backend.metadata(path).is_ok() {
        return Err(ErrNo::EADDRINUSE);
    }
    match vfs::mknod_socket_absolute(path) {
        Ok(()) => Ok(()),
        Err(VfsError::Exists) => Err(ErrNo::EADDRINUSE),
        Err(VfsError::NotFound) => Err(ErrNo::ENOENT),
        Err(VfsError::NotAFile) => Err(ErrNo::ENOTDIR),
        Err(e) => Err(vfs_error_to_errno(e)),
    }
}

impl VfsIoHandle for UnixSocketHandle {
    fn open_accmode(&self) -> u32 { 2 }

    fn prepare_read(&mut self, max_len: usize) -> VfsResult<Box<dyn VfsPreparedRead>> {
        let inner = self.sock.inner.lock();
        if let Some(mut endpoint) = inner.endpoint.clone() {
            drop(inner);
            return endpoint.prepare_read(max_len);
        }
        if inner.sock_type != UnixSockType::Dgram {
            return Err(VfsError::Unsupported);
        }
        let inbox = inner.dgram_inbox.clone().ok_or(VfsError::Unsupported)?;
        Ok(Box::new(DgramPreparedRead {
            inbox,
            nonblocking: inner.nonblocking,
            max_len,
        }))
    }

    fn open_status_flags(&self) -> u32 {
        if self.sock.inner.lock().nonblocking {
            SOCK_NONBLOCK as u32
        } else {
            0
        }
    }

    fn set_open_status_flags(&mut self, flags : u32) -> VfsResult<()> {
        let nonblocking = flags as usize & SOCK_NONBLOCK != 0;
        let mut inner = self.sock.inner.lock();
        inner.nonblocking = nonblocking;
        if let Some(endpoint) = inner.endpoint.as_mut() {
            endpoint.set_open_status_flags(if nonblocking {
                                              SOCK_NONBLOCK as u32
                                          } else {
                                              0
                                          })?;
        }
        Ok(())
    }

    fn read(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        let prepared = self.prepare_read(buf.len())?;
        let lease = prepared.acquire()?;
        let n = lease.bytes().len();
        buf[..n].copy_from_slice(lease.bytes());
        match lease.finish(VfsCopyProgress {
            copied: n,
            complete: true,
        })? {
            VfsReadFinish::Bytes(copied) => Ok(copied),
            VfsReadFinish::Fault => Err(VfsError::Io),
        }
    }

    fn write(&mut self, buf: &[u8]) -> VfsResult<usize> {
        let sock = self.sock.clone();
        let inner = sock.inner.lock();
        if let Some(mut endpoint) = inner.endpoint.clone() {
            drop(inner);
            return endpoint.write(buf);
        }
        if inner.sock_type == UnixSockType::Dgram {
            if let Some(peer) = inner.dgram_peer_inbox.clone() {
                let sender = inner.bound_key.clone();
                drop(inner);
                return peer.push(DgramPacket { data: buf.to_vec(),
                                               sender })
                           .map(|()| buf.len())
                           .map_err(map_errno);
            }
            let peer = inner.dgram_peer.clone().ok_or(VfsError::Unsupported)?;
            let sender = inner.bound_key.clone();
            drop(inner);
            return deliver_dgram(&peer, buf, sender).map_err(map_errno);
        }
        Err(VfsError::Unsupported)
    }

    fn poll_revents(&mut self, events: i16) -> VfsResult<i16> {
        let mut inner = self.sock.inner.lock();
        if let Some(endpoint) = inner.endpoint.as_mut() {
            return endpoint.poll_revents(events);
        }
        const POLLIN: i16 = 0x001;
        const POLLOUT: i16 = 0x004;
        let mut revents = 0i16;
        if events & POLLIN != 0 {
            let has_data = inner
                .dgram_inbox
                .as_ref()
                .is_some_and(|inbox| inbox.has_data());
            if has_data {
                revents |= POLLIN;
            }
        }
        if events & POLLOUT != 0 {
            revents |= POLLOUT;
        }
        Ok(revents)
    }

    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(VfsMetadata {
            node_type: VfsNodeType::Special,
            size: 0,
            mode: 0o140600,
            inode: self.inode,
            mount_id: 0,
            nlink: 1,
            device_major: 0,
            device_minor: 0,
            uid: 0,
            gid: 0,
        })
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(Self {
            sock: self.sock.clone(),
            inode: self.inode,
        }))
    }
}

struct DgramPreparedRead {
    inbox: Arc<DgramInbox>,
    nonblocking: bool,
    max_len: usize,
}

impl VfsPreparedRead for DgramPreparedRead {
    fn acquire(self: Box<Self>) -> VfsResult<Box<dyn VfsReadLease>> {
        let lease = self.inbox.acquire(self.nonblocking)?;
        Ok(Box::new(DgramVfsReadLease {
            lease: Some(lease),
            max_len: self.max_len,
        }))
    }
}

struct DgramVfsReadLease {
    lease: Option<DgramReadLease>,
    max_len: usize,
}

impl VfsReadLease for DgramVfsReadLease {
    fn bytes(&self) -> &[u8] {
        self.lease
            .as_ref()
            .map(|lease| lease.bytes(self.max_len))
            .unwrap_or(&[])
    }

    fn finish(mut self: Box<Self>, progress: VfsCopyProgress) -> VfsResult<VfsReadFinish> {
        self.lease
            .take()
            .ok_or(VfsError::Io)?
            .finish(progress.copied, progress.complete)
    }
}

fn map_errno(errno: ErrNo) -> VfsError {
    match errno {
        ErrNo::EAGAIN => VfsError::WouldBlock,
        ErrNo::ENOTCONN => VfsError::Unsupported,
        _ => VfsError::Io,
    }
}

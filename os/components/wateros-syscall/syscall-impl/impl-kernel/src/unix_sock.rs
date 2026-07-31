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
use vfs::api::{SingleRootReadView, VfsError, VfsMetadata, VfsNodeType, VfsResult};
use vfs::UnixStreamPairEnd;

use crate::socket_block::socket_blocking_tick;
use crate::user_copy::{copy_from_user, copy_to_user, copy_to_user_struct};
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
    inbox: VecDeque<Vec<u8>>,
}

struct BoundEntry {
    sock_type: UnixSockType,
    listening: bool,
    accept_queue: VecDeque<UnixStreamPairEnd>,
    dgram_inbox: VecDeque<(Vec<u8>, Option<Vec<u8>>)>,
}

static NEXT_INODE: AtomicU64 = AtomicU64::new(0x4_0000);
static FD_TABLE: Mutex<BTreeMap<(usize, usize), UnixSockRef>> = Mutex::new(BTreeMap::new());
static BOUND: Mutex<BTreeMap<Vec<u8>, BoundEntry>> = Mutex::new(BTreeMap::new());

pub(crate) struct UnixSocketHandle {
    sock: UnixSockRef,
    inode: u64,
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
    let key = sock.inner.lock().bound_key.clone();
    let still_referenced = table
        .values()
        .any(|other| Arc::ptr_eq(&other.inner, &sock.inner));
    drop(sock);
    if let Some(key) = key {
        if !still_referenced {
            BOUND.lock().remove(&key);
        }
    }
}

pub(crate) fn copy_fds_from_parent(child: usize, parent: usize) {
    let mut table = FD_TABLE.lock();
    let inherited: Vec<_> = table
        .iter()
        .filter_map(|(&(owner, fd), sock)| {
            if owner == parent {
                Some(((child, fd), sock.clone()))
            } else {
                None
            }
        })
        .collect();
    for (key, sock) in inherited {
        table.insert(key, sock);
    }
}

pub(crate) fn drop_task(task_id: usize) {
    let fds: Vec<usize> = FD_TABLE
        .lock()
        .keys()
        .filter(|(owner, _)| *owner == task_id)
        .map(|(_, fd)| *fd)
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
    let sock = UnixSockRef {
        inner: Arc::new(Mutex::new(UnixSockInner {
            sock_type,
            nonblocking,
            bound_key: None,
            peer_key: None,
            listening: false,
            endpoint: None,
            dgram_peer: None,
            inbox: VecDeque::new(),
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
                inbox: VecDeque::new(),
            })),
        };
        let inode = NEXT_INODE.fetch_add(1, Ordering::Relaxed);
        let handle: Box<dyn VfsIoHandle> =
            Box::new(UnixSocketHandle { sock: sock.clone(), inode });
        (handle, sock)
    };
    (make_socket(endpoint0), make_socket(endpoint1))
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
    bound.insert(
        addr.key.clone(),
        BoundEntry {
            sock_type: inner.sock_type,
            listening: false,
            accept_queue: VecDeque::new(),
            dgram_inbox: VecDeque::new(),
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
                    inbox: VecDeque::new(),
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
    let key = if addr_ptr != 0 && addrlen >= 2 {
        parse_sockaddr_un(addr_ptr, addrlen)?.key
    } else {
        inner.dgram_peer.clone().ok_or(ErrNo::ENOTCONN)?
    };
    if inner.sock_type != UnixSockType::Dgram {
        return Err(ErrNo::EOPNOTSUPP);
    }
    drop(inner);
    let mut bound = BOUND.lock();
    let entry = bound.get_mut(&key).ok_or(ErrNo::ECONNREFUSED)?;
    if entry.sock_type != UnixSockType::Dgram {
        return Err(ErrNo::ECONNREFUSED);
    }
    if entry.dgram_inbox.len() >= UNIX_DGRAM_INBOX_MAX {
        log::warn!("[unix_sock] dgram_inbox full key_len={} cap={}",
                   key.len(),
                   UNIX_DGRAM_INBOX_MAX);
        return Err(ErrNo::EAGAIN);
    }
    entry.dgram_inbox.push_back((buf.to_vec(), sender_key));
    Ok(buf.len())
}

pub(crate) fn recvfrom_unix(
    fd: usize,
    buf: &mut [u8],
    addr_ptr: usize,
    addrlen_ptr: usize,
) -> Result<usize, ErrNo> {
    let sock = lookup_current(fd)?;
    loop {
        let packet = {
            let mut inner = sock.inner.lock();
            if let Some(mut endpoint) = inner.endpoint.clone() {
                drop(inner);
                return endpoint.read(buf).map_err(vfs_error_to_errno);
            }
            if let Some(packet) = inner.inbox.pop_front() {
                Some((packet, None))
            } else if let Some(key) = inner.bound_key.clone() {
                let mut bound = BOUND.lock();
                bound
                    .get_mut(&key)
                    .and_then(|entry| entry.dgram_inbox.pop_front())
            } else {
                None
            }
        };
        if let Some((packet, sender)) = packet {
            let n = packet.len().min(buf.len());
            buf[..n].copy_from_slice(&packet[..n]);
            if addr_ptr != 0 && addrlen_ptr != 0 {
                if let Some(sender_key) = sender {
                    let _ = write_unix_addr_to_user(addr_ptr, addrlen_ptr, &sender_key);
                }
            }
            return Ok(n);
        }
        let nonblocking = sock.inner.lock().nonblocking;
        if nonblocking {
            return Err(ErrNo::EAGAIN);
        }
        task::sleep_for_ticks(1);
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

fn pop_dgram_packet(inner: &mut UnixSockInner) -> Option<(Vec<u8>, Option<Vec<u8>>)> {
    if let Some(packet) = inner.inbox.pop_front() {
        return Some((packet, None));
    }
    let key = inner.bound_key.clone()?;
    BOUND.lock()
        .get_mut(&key)
        .and_then(|entry| entry.dgram_inbox.pop_front())
}

fn deliver_dgram(peer: &[u8], buf: &[u8], sender: Option<Vec<u8>>) -> Result<usize, ErrNo> {
    let mut bound = BOUND.lock();
    let entry = bound.get_mut(peer).ok_or(ErrNo::ECONNREFUSED)?;
    if entry.sock_type != UnixSockType::Dgram {
        return Err(ErrNo::ECONNREFUSED);
    }
    if entry.dgram_inbox.len() >= UNIX_DGRAM_INBOX_MAX {
        log::warn!("[unix_sock] dgram_inbox full peer_len={} cap={}",
                   peer.len(),
                   UNIX_DGRAM_INBOX_MAX);
        return Err(ErrNo::EAGAIN);
    }
    entry.dgram_inbox.push_back((buf.to_vec(), sender));
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

    fn read(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        let sock = self.sock.clone();
        loop {
            let packet = {
                let mut inner = sock.inner.lock();
                if let Some(mut endpoint) = inner.endpoint.clone() {
                    drop(inner);
                    return endpoint.read(buf);
                }
                if inner.sock_type == UnixSockType::Dgram {
                    if let Some((packet, _)) = pop_dgram_packet(&mut inner) {
                        let n = packet.len().min(buf.len());
                        buf[..n].copy_from_slice(&packet[..n]);
                        return Ok(n);
                    }
                    if inner.nonblocking {
                        return Err(VfsError::WouldBlock);
                    }
                } else if inner.nonblocking {
                    return Err(VfsError::WouldBlock);
                }
            };
            let _ = packet;
            task::sleep_for_ticks(1);
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
            let has_data = !inner.inbox.is_empty()
                || inner.bound_key.as_ref().is_some_and(|key| {
                    BOUND
                        .lock()
                        .get(key)
                        .is_some_and(|entry| !entry.dgram_inbox.is_empty())
                });
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

fn map_errno(errno: ErrNo) -> VfsError {
    match errno {
        ErrNo::EAGAIN => VfsError::WouldBlock,
        ErrNo::ENOTCONN => VfsError::Unsupported,
        _ => VfsError::Io,
    }
}

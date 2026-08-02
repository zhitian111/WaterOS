//! 文件 I/O 操作：`read`/`readv`、`write`/`writev`、`pread64`/`pwrite64`/`preadv`/`pwritev`/`lseek`。

extern crate alloc;
use core::sync::atomic::{AtomicU64, Ordering};
use alloc::vec::Vec;
use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use network::stack;
use vfs::api::{VfsCopyProgress, VfsError, VfsReadFinish, VfsSeekWhence};
use crate::fallible_buf::{try_kbuf, SYSCALL_IO_MAX};
use crate::socket_block::socket_blocking_tick;
use crate::socket_fd;
use crate::user_copy::{
    copy_from_user, copy_from_user_struct, copy_to_user, copy_to_user_progress,
};
use crate::vfs_util::{vfs_error_to_errno, vfs_io_at_error_to_errno};

const SMALL_READ_BUF_SIZE: usize = 256;
const MAX_IO: usize = 0x7ffff000;
const IO_CHUNK: usize = 64 * 1024;
const TCP_BULK_WRITE_YIELD_THRESHOLD: usize = 4096;
const TCP_MSS_BYTES: usize = 1460;
const TCP_LOOPBACK_POLL_ROUNDS: usize = 4;
const UDP_SMALL_WRITE_YIELD_THRESHOLD: usize = 256;
const UDP_BULK_WRITE_YIELD_INTERVAL: u64 = 4;

static UDP_BULK_WRITE_COUNT: AtomicU64 = AtomicU64::new(0);

const SEEK_SET: u32 = 0;
const SEEK_CUR: u32 = 1;
const SEEK_END: u32 = 2;

/// 用户态 iovec 结构（与 Linux `struct iovec` 布局一致）。
#[repr(C)]
#[derive(Clone, Copy)]
struct UserIoVec {
    base: usize,
    len: usize,
}


pub(crate) fn sys_read(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let ptr = args.arg(1);
    let len = args.arg(2);
    if let Err(err) = validate_read_fd(fd) {
        return UserRet::from_error(err);
    }
    if len == 0 {
        return UserRet::from_success(0);
    }
    if ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let transfer_len = read_transfer_len(len);
    match read_fd_prepared(fd, ptr, transfer_len) {
        Ok(Some(ret)) => return ret,
        Ok(None) => {}
        Err(err) => return UserRet::from_error(err),
    }
    read_fd_legacy(fd, ptr, transfer_len)
}

/// Run lease-capable sources without retaining the fd-slot lock. RIO-05 through
/// RIO-08 replace the explicit legacy branch for stream and device handles.
fn read_fd_prepared(fd : usize,
                    ptr : usize,
                    transfer_len : usize)
                    -> Result<Option<UserRet>, ErrNo> {
    let prepared = loop {
        match vfs::fd::prepare_current_read(fd, transfer_len) {
            Ok(prepared) => break prepared,
            Err(VfsError::Busy) => task::yield_now(),
            Err(VfsError::Unsupported) => return Ok(None),
            Err(error) => return Err(vfs_error_to_errno(error)),
        }
    };
    let lease = prepared.acquire().map_err(vfs_error_to_errno)?;
    let progress = copy_to_user_progress(ptr, lease.bytes());
    let finish = lease.finish(VfsCopyProgress {
                          copied : progress.copied,
                          complete : progress.error.is_none(),
                      })
                      .map_err(vfs_error_to_errno)?;
    Ok(Some(match finish {
        VfsReadFinish::Bytes(copied) => UserRet::from_success(copied),
        VfsReadFinish::Fault => UserRet::from_error(progress.error.unwrap_or(ErrNo::EFAULT)),
    }))
}

fn read_fd_legacy(fd : usize, ptr : usize, transfer_len : usize) -> UserRet {
    if transfer_len <= SMALL_READ_BUF_SIZE {
        let mut kbuf = [0u8; SMALL_READ_BUF_SIZE];
        let n = match read_fd(fd, &mut kbuf[..transfer_len]) {
            Ok(n) => n,
            Err(err) => return UserRet::from_error(err),
        };
        return do_finish_read(ptr, &kbuf[..n], n);
    }
    let mut kbuf = match try_kbuf(transfer_len, SYSCALL_IO_MAX) {
        Ok(buf) => buf,
        Err(err) => return UserRet::from_error(err),
    };
    let n = match read_fd(fd, &mut kbuf) {
        Ok(n) => n,
        Err(err) => return UserRet::from_error(err),
    };
    do_finish_read(ptr, &kbuf[..n], n)
}

fn validate_read_fd(fd : usize) -> Result<(), ErrNo> {
    if socket_fd::lookup(fd).is_some() {
        return Ok(());
    }
    if vfs::fd::is_path_only_fd(fd).map_err(vfs_error_to_errno)? {
        return Err(ErrNo::EBADF);
    }
    vfs::fd::with_current_io(fd, |handle| handle.validate_read_access())
        .map_err(vfs_error_to_errno)
}

/// 内核缓冲区有独立上限；大 `count` 通过合法短读分批完成，不能返回 `EINVAL`。
fn read_transfer_len(requested : usize) -> usize {
    requested.min(MAX_IO)
             .min(SYSCALL_IO_MAX)
}

// 本方法代码由AI完成
pub(crate) fn sys_readv(args : SyscallArgs) -> UserRet {
    // iozone 调试：最早期 trace，在参数解包前
    let fd = args.arg(0);
    let iov_ptr = args.arg(1);
    let iovcnt = args.arg(2);
    let arg4 = args.arg(3);
    let arg5 = args.arg(4);
    let arg6 = args.arg(5);
    log::trace!("[sys_readv] RAW fd={} iov_ptr={:#x} iovcnt={:#x}({}) a3={:#x} a4={:#x} a5={:#x}",
                fd,
                iov_ptr,
                iovcnt,
                iovcnt,
                arg4,
                arg5,
                arg6);
    if iovcnt == 0 {
        return UserRet::from_success(0);
    }
    if iov_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if iovcnt > 1024 {
        log::trace!("[sys_readv] EINVAL iovcnt={} > 1024 (BUG: iozone passed garbage iovcnt)",
                    iovcnt);
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let iov_size = core::mem::size_of::<UserIoVec>();
    let mut total = 0usize;
    for i in 0..iovcnt {
        let iov = match copy_from_user_struct::<UserIoVec>(iov_ptr + i * iov_size) {
            Ok(v) => v,
            Err(e) => return UserRet::from_error(e),
        };
        if iov.len == 0 {
            continue;
        }
        if iov.base == 0 {
            return UserRet::from_error(ErrNo::EFAULT);
        }
        if iov.len > SYSCALL_IO_MAX {
            return UserRet::from_error(ErrNo::EINVAL);
        }
        if total.checked_add(iov.len)
                .is_none()
        {
            return UserRet::from_error(ErrNo::EINVAL);
        }

        // iozone 调试：每个 iov 段读取前
        log::trace!("[sys_readv] iov[{}/{}] base={:#x} len={} total_before={} -> \
                     read_fd(fd={})...",
                    i,
                    iovcnt,
                    iov.base,
                    iov.len,
                    total,
                    fd);
        let mut kbuf = match try_kbuf(iov.len, SYSCALL_IO_MAX) {
            Ok(buf) => buf,
            Err(err) => {
                log::trace!("[sys_readv] iov[{}/{}] alloc ERR fd={} err={:?} total={}",
                            i,
                            iovcnt,
                            fd,
                            err,
                            total);
                return if total > 0 {
                    UserRet::from_success(total)
                } else {
                    UserRet::from_error(err)
                };
            }
        };
        let n = match read_fd(fd, &mut kbuf) {
            Ok(n) => n,
            Err(err) => {
                log::trace!("[sys_readv] iov[{}/{}] read_fd ERR fd={} err={:?} total={}",
                            i,
                            iovcnt,
                            fd,
                            err,
                            total);
                return if total > 0 {
                    UserRet::from_success(total)
                } else {
                    UserRet::from_error(err)
                };
            }
        };
        // iozone 调试：read_fd 返回后
        log::trace!("[sys_readv] iov[{}/{}] read_fd OK fd={} n={} (want {})",
                    i,
                    iovcnt,
                    fd,
                    n,
                    iov.len);
        if n == 0 {
            return UserRet::from_success(total);
        }
        match copy_to_user(iov.base, &kbuf[..n]) {
            Ok(written) if written == n => {}
            _ => return UserRet::from_error(ErrNo::EFAULT),
        }
        total += n;
        if n < iov.len {
            log::trace!("[sys_readv] iov[{}/{}] short read n={} < len={} total={} EXIT",
                        i,
                        iovcnt,
                        n,
                        iov.len,
                        total);
            return UserRet::from_success(total);
        }
    }

    log::trace!("[sys_readv] EXIT total={}", total);
    UserRet::from_success(total)
}

/// 将内核缓冲区的数据拷贝到用户空间并返回结果。
fn do_finish_read(ptr : usize, kbuf : &[u8], n : usize) -> UserRet {
    if n == 0 {
        return UserRet::from_success(0);
    }
    match copy_to_user(ptr, &kbuf[..n]) {
        Ok(w) if w == n => UserRet::from_success(n),
        _ => UserRet::from_error(ErrNo::EFAULT),
    }
}

fn read_fd(fd : usize, buf : &mut [u8]) -> Result<usize, ErrNo> {
    if let Some(socket) = socket_fd::lookup(fd) {
        return match stack::socket_kind(socket.handle()) {
            Ok(stack::SocketKind::Tcp) => read_tcp_socket_blocking(fd, buf),
            Ok(stack::SocketKind::Udp) => read_udp_socket_blocking(fd, buf),
            Err(_) => Err(ErrNo::ENOTSOCK),
        };
    }
    if vfs::fd::is_path_only_fd(fd).map_err(vfs_error_to_errno)? {
        return Err(ErrNo::EBADF);
    }
    // iozone 调试：VFS read 调用前
    log::trace!("[read_fd] vfs_read fd={} len={}",
                fd,
                buf.len());
    let result =
        vfs::fd::with_current_io(fd, |handle| handle.read(buf)).map_err(vfs_error_to_errno);
    // iozone 调试：VFS read 返回后
    match &result {
        Ok(n) => log::trace!("[read_fd] vfs_read OK fd={} n={}/{}",
                             fd,
                             n,
                             buf.len()),
        Err(e) => log::trace!("[read_fd] vfs_read ERR fd={} err={:?}",
                              fd,
                              e),
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_transfer_len_preserves_small_requests() {
        assert_eq!(read_transfer_len(4096), 4096);
    }

    #[test]
    fn read_transfer_len_turns_large_requests_into_short_reads() {
        assert_eq!(read_transfer_len(SYSCALL_IO_MAX + 1), SYSCALL_IO_MAX);
        assert_eq!(read_transfer_len(usize::MAX), SYSCALL_IO_MAX);
    }

    #[test]
    fn write_transfer_len_preserves_small_requests() {
        assert_eq!(write_transfer_len(4096), 4096);
    }

    #[test]
    fn write_transfer_len_turns_large_requests_into_short_writes() {
        assert_eq!(write_transfer_len(SYSCALL_IO_MAX + 1), SYSCALL_IO_MAX);
        assert_eq!(write_transfer_len(usize::MAX), SYSCALL_IO_MAX);
    }
}

fn read_tcp_socket_blocking(fd : usize, buf : &mut [u8]) -> Result<usize, ErrNo> {
    let nonblocking = socket_fd::is_nonblocking(fd);
    let task_id = task::current_task_id().unwrap_or(0);
    loop {
        let socket = socket_fd::lookup(fd).ok_or(ErrNo::ENOTSOCK)?;
        let handle = socket.handle();
        drive_network_stack();
        let can_recv = stack::socket_can_recv(handle).unwrap_or(false);
        let may_recv = stack::socket_may_recv(handle).unwrap_or(false);
        let state = stack::socket_state(handle);
        if can_recv {
            match stack::socket_recv(handle, buf) {
                Ok(n) => return Ok(n),
                Err(_) => return Err(ErrNo::EIO),
            }
        }
        if !may_recv {
            return Ok(0);
        }
        if matches!(state, Ok(stack::SocketState::Closed)) {
            return Ok(0);
        }
        socket_blocking_tick(nonblocking, task_id)?;
    }
}

fn read_udp_socket_blocking(fd : usize, buf : &mut [u8]) -> Result<usize, ErrNo> {
    let nonblocking = socket_fd::is_nonblocking(fd);
    let task_id = task::current_task_id().unwrap_or(0);
    loop {
        let socket = socket_fd::lookup(fd).ok_or(ErrNo::ENOTSOCK)?;
        let handle = socket.handle();
        drive_network_stack();
        if stack::socket_udp_can_recv(handle).unwrap_or(false) {
            return stack::socket_recv(handle, buf).map_err(|_| ErrNo::EIO);
        }
        socket_blocking_tick(nonblocking, task_id)?;
    }
}

fn drive_network_stack() {
    match platform::timer::now_duration() {
        Ok(now) => {
            let millis = now.as_millis()
                            .min(i64::MAX as u128) as i64;
            stack::poll_at_millis(millis);
        }
        Err(_) => stack::poll(),
    }
    stack::poll_socket_events();
}

fn flush_segmented_loopback_send(handle : stack::StackSocketHandle, sent : usize) {
    if sent <= TCP_MSS_BYTES || !stack::socket_peer_is_loopback(handle).unwrap_or(false) {
        return;
    }
    for _ in 0..TCP_LOOPBACK_POLL_ROUNDS {
        drive_network_stack();
    }
}

pub(crate) fn sys_write(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let ptr = args.arg(1);
    let len = args.arg(2);
    if len == 0 {
        return UserRet::from_success(0);
    }
    if ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let transfer_len = write_transfer_len(len);
    let mut kbuf = match try_kbuf(transfer_len, SYSCALL_IO_MAX) {
        Ok(buf) => buf,
        Err(err) => return UserRet::from_error(err),
    };
    match copy_from_user(&mut kbuf, ptr) {
        Ok(n) if n == transfer_len => {}
        _ => return UserRet::from_error(ErrNo::EFAULT),
    }
    // iozone 调试：每 128 次 write 才打一次 trace，避免刷屏淹没 readv 日志
    {
        static WRITE_COUNT : core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);
        let cnt = WRITE_COUNT.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        if cnt % 128 == 0 {
            log::trace!("[sys_write] cnt={} fd={} len={} ptr={:#x}",
                        cnt,
                        fd,
                        transfer_len,
                        ptr);
        }
    }
    let result = write_fd(fd, &kbuf);
    let ret = match result {
        Ok(n) => {
            log::trace!("[sys_write] OK fd={} n={}", fd, n);
            UserRet::from_success(n)
        }
        Err(ErrNo::EPIPE) => {
            let _ = crate::sys::ipc::signal::raise_current_thread(ipc::signal::SIGPIPE);
            UserRet::from_error(ErrNo::EPIPE)
        }
        Err(err) => {
            log::trace!("[sys_write] ERR fd={} err={:?}",
                        fd,
                        err);
            UserRet::from_error(err)
        }
    };
    ret
}

/// `MAX_RW_COUNT` is the ABI limit while `SYSCALL_IO_MAX` only bounds kernel
/// staging. Requests larger than staging capacity must make progress via a
/// legal short write instead of failing with `EINVAL`.
fn write_transfer_len(requested : usize) -> usize {
    requested.min(MAX_IO)
             .min(SYSCALL_IO_MAX)
}

// 本方法代码由AI完成
pub(crate) fn sys_writev(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let iov_ptr = args.arg(1);
    let iovcnt = args.arg(2);
    if iovcnt == 0 {
        return UserRet::from_success(0);
    }
    if iov_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if iovcnt > 1024 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let iov_size = core::mem::size_of::<UserIoVec>();
    let mut out = Vec::new();
    for i in 0..iovcnt {
        let iov_addr = match i.checked_mul(iov_size)
                              .and_then(|offset| iov_ptr.checked_add(offset))
        {
            Some(addr) => addr,
            None => return UserRet::from_error(ErrNo::EFAULT),
        };
        let iov = match copy_from_user_struct::<UserIoVec>(iov_addr) {
            Ok(v) => v,
            Err(e) => return UserRet::from_error(e),
        };
        if iov.len == 0 {
            continue;
        }
        if iov.base == 0 {
            return UserRet::from_error(ErrNo::EFAULT);
        }
        let remaining = SYSCALL_IO_MAX - out.len();
        if remaining == 0 {
            break;
        }
        let segment_len = iov.len.min(remaining);
        let new_len = out.len() + segment_len;
        let old_len = out.len();
        if out.try_reserve_exact(new_len - old_len).is_err() {
            return UserRet::from_error(ErrNo::ENOMEM);
        }
        out.resize(new_len, 0);
        match copy_from_user(&mut out[old_len..], iov.base) {
            Ok(n) if n == segment_len => {}
            _ => return UserRet::from_error(ErrNo::EFAULT),
        }
        if segment_len < iov.len {
            break;
        }
    }

    match write_fd(fd, &out) {
        Ok(n) => UserRet::from_success(n),
        Err(ErrNo::EPIPE) => {
            let _ = crate::sys::ipc::signal::raise_current_thread(ipc::signal::SIGPIPE);
            UserRet::from_error(ErrNo::EPIPE)
        }
        Err(err) => UserRet::from_error(err),
    }
}

fn write_fd(fd : usize, buf : &[u8]) -> Result<usize, ErrNo> {
    if let Some(socket) = socket_fd::lookup(fd) {
        return match stack::socket_kind(socket.handle()) {
            Ok(stack::SocketKind::Tcp) => write_tcp_socket_blocking(fd, buf),
            Ok(stack::SocketKind::Udp) => write_udp_socket_blocking(fd, buf),
            Err(_) => Err(ErrNo::ENOTSOCK),
        };
    }
    if vfs::fd::is_path_only_fd(fd).map_err(vfs_error_to_errno)? {
        return Err(ErrNo::EBADF);
    }
    loop {
        match vfs::fd::with_current_io(fd, |handle| handle.write(buf)) {
            Err(VfsError::Busy) => task::yield_now(),
            result => return result.map_err(vfs_error_to_errno),
        }
    }
}

fn write_tcp_socket_blocking(fd : usize, buf : &[u8]) -> Result<usize, ErrNo> {
    let nonblocking = socket_fd::is_nonblocking(fd);
    let task_id = task::current_task_id().unwrap_or(0);
    loop {
        let socket = socket_fd::lookup(fd).ok_or(ErrNo::ENOTSOCK)?;
        let handle = socket.handle();
        drive_network_stack();
        let may_send = stack::socket_may_send(handle).unwrap_or(false);
        let send_capacity = stack::socket_send_capacity(handle).unwrap_or(0);
        let connected = stack::socket_is_connected(handle).unwrap_or(false);
        if may_send && send_capacity > 0 {
            let send_len = buf.len()
                              .min(send_capacity);
            match stack::socket_send(handle, &buf[..send_len]) {
                Ok(n) if n > 0 => {
                    flush_segmented_loopback_send(handle, n);
                    if n >= TCP_BULK_WRITE_YIELD_THRESHOLD {
                        drive_network_stack();
                        task::yield_now();
                        drive_network_stack();
                    }
                    return Ok(n);
                }
                Ok(_) => {}
                Err(_) => {
                    return Err(ErrNo::EIO);
                }
            }
        }
        if !connected {
            return Err(ErrNo::EPIPE);
        }
        socket_blocking_tick(nonblocking, task_id)?;
    }
}

fn write_udp_socket_blocking(fd : usize, buf : &[u8]) -> Result<usize, ErrNo> {
    let nonblocking = socket_fd::is_nonblocking(fd);
    let task_id = task::current_task_id().unwrap_or(0);
    loop {
        let socket = socket_fd::lookup(fd).ok_or(ErrNo::ENOTSOCK)?;
        drive_network_stack();
        match stack::socket_send(socket.handle(), buf) {
            Ok(n) => {
                let should_yield = n <= UDP_SMALL_WRITE_YIELD_THRESHOLD ||
                                   UDP_BULK_WRITE_COUNT.fetch_add(1, Ordering::Relaxed) %
                                   UDP_BULK_WRITE_YIELD_INTERVAL ==
                                   0;
                if should_yield {
                    drive_network_stack();
                    task::yield_now();
                    drive_network_stack();
                }
                return Ok(n);
            }
            Err(stack::SocketSendError::WouldBlock) => {
                socket_blocking_tick(nonblocking, task_id)?;
            }
            Err(err) => {
                return Err(crate::sys::net::sendto::socket_send_error_to_errno(err));
            }
        }
    }
}

fn offset_from_arg(raw : usize) -> Result<u64, ErrNo> {
    let off = raw as i64;
    if off < 0 {
        return Err(ErrNo::EINVAL);
    }
    Ok(off as u64)
}

fn gather_user_iovecs(iov_ptr : usize, iovcnt : usize) -> Result<Vec<u8>, ErrNo> {
    if iovcnt == 0 {
        return Ok(Vec::new());
    }
    if iov_ptr == 0 {
        return Err(ErrNo::EFAULT);
    }
    if iovcnt > 1024 {
        return Err(ErrNo::EINVAL);
    }

    let iov_size = core::mem::size_of::<UserIoVec>();
    let mut out = Vec::new();
    for i in 0..iovcnt {
        let iov_addr = i.checked_mul(iov_size)
                        .and_then(|offset| iov_ptr.checked_add(offset))
                        .ok_or(ErrNo::EFAULT)?;
        let iov = copy_from_user_struct::<UserIoVec>(iov_addr)?;
        if iov.len == 0 {
            continue;
        }
        if iov.base == 0 {
            return Err(ErrNo::EFAULT);
        }
        let remaining = SYSCALL_IO_MAX - out.len();
        if remaining == 0 {
            break;
        }
        let segment_len = iov.len.min(remaining);
        let new_len = out.len() + segment_len;
        let old_len = out.len();
        out.try_reserve_exact(segment_len)
           .map_err(|_| ErrNo::ENOMEM)?;
        out.resize(new_len, 0);
        match copy_from_user(&mut out[old_len..], iov.base) {
            Ok(n) if n == segment_len => {}
            _ => return Err(ErrNo::EFAULT),
        }
        if segment_len < iov.len {
            break;
        }
    }
    Ok(out)
}

fn scatter_to_user_iovecs(iov_ptr : usize, iovcnt : usize, data : &[u8]) -> Result<usize, ErrNo> {
    if data.is_empty() {
        return Ok(0);
    }
    if iovcnt == 0 {
        return Ok(0);
    }
    if iov_ptr == 0 {
        return Err(ErrNo::EFAULT);
    }

    let iov_size = core::mem::size_of::<UserIoVec>();
    let mut written = 0usize;
    let mut src_off = 0usize;
    for i in 0..iovcnt {
        if src_off >= data.len() {
            break;
        }
        let iov = copy_from_user_struct::<UserIoVec>(iov_ptr + i * iov_size)?;
        if iov.len == 0 {
            continue;
        }
        if iov.base == 0 {
            return Err(ErrNo::EFAULT);
        }
        let n = iov.len
                   .min(data.len() - src_off);
        copy_to_user(iov.base, &data[src_off..src_off + n])?;
        src_off += n;
        written += n;
    }
    Ok(written)
}

fn total_iov_len(iov_ptr : usize, iovcnt : usize) -> Result<usize, ErrNo> {
    if iovcnt == 0 {
        return Ok(0);
    }
    if iov_ptr == 0 {
        return Err(ErrNo::EFAULT);
    }
    if iovcnt > 1024 {
        return Err(ErrNo::EINVAL);
    }
    let iov_size = core::mem::size_of::<UserIoVec>();
    let mut total = 0usize;
    for i in 0..iovcnt {
        let iov = copy_from_user_struct::<UserIoVec>(iov_ptr + i * iov_size)?;
        total = total.checked_add(iov.len)
                     .ok_or(ErrNo::EINVAL)?;
        if total > MAX_IO {
            return Err(ErrNo::EINVAL);
        }
    }
    Ok(total)
}

// 本方法代码由AI完成
pub(crate) fn sys_pread64(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let ptr = args.arg(1);
    let len = args.arg(2);
    if len == 0 {
        return UserRet::from_success(0);
    }
    if ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if len > MAX_IO {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let offset = match offset_from_arg(args.arg(3)) {
        Ok(v) => v,
        Err(e) => return UserRet::from_error(e),
    };

    let mut kbuf = Vec::with_capacity(len);
    kbuf.resize(len, 0);
    let n = match vfs::fd::with_current_io(fd, |handle| {
              handle.read_at(offset, &mut kbuf)
          }) {
        Ok(n) => n,
        Err(err) => return UserRet::from_error(vfs_io_at_error_to_errno(err)),
    };
    if n == 0 {
        return UserRet::from_success(0);
    }
    match copy_to_user(ptr, &kbuf[..n]) {
        Ok(w) if w == n => UserRet::from_success(n),
        _ => UserRet::from_error(ErrNo::EFAULT),
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_pwrite64(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let ptr = args.arg(1);
    let len = args.arg(2);
    if len == 0 {
        return UserRet::from_success(0);
    }
    if ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let offset = match offset_from_arg(args.arg(3)) {
        Ok(v) => v,
        Err(e) => return UserRet::from_error(e),
    };

    let transfer_len = write_transfer_len(len);
    let mut kbuf = match try_kbuf(transfer_len, SYSCALL_IO_MAX) {
        Ok(buf) => buf,
        Err(err) => return UserRet::from_error(err),
    };
    match copy_from_user(&mut kbuf, ptr) {
        Ok(n) if n == transfer_len => {}
        _ => return UserRet::from_error(ErrNo::EFAULT),
    }
    match vfs::fd::with_current_io(fd, |handle| {
              handle.write_at(offset, &kbuf)
          }) {
        Ok(n) => UserRet::from_success(n),
        Err(err) => UserRet::from_error(vfs_io_at_error_to_errno(err)),
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_preadv(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let iov_ptr = args.arg(1);
    let iovcnt = args.arg(2);
    let want = match total_iov_len(iov_ptr, iovcnt) {
        Ok(v) => v,
        Err(e) => return UserRet::from_error(e),
    };
    if want == 0 {
        return UserRet::from_success(0);
    }
    let offset = match offset_from_arg(args.arg(3)) {
        Ok(v) => v,
        Err(e) => return UserRet::from_error(e),
    };

    log::info!("[sys_preadv] fd={} want={} offset={}",
               fd,
               want,
               offset);
    let mut file_off = offset;
    let mut gathered = Vec::new();
    let mut remaining = want;
    while remaining > 0 {
        let chunk = remaining.min(IO_CHUNK);
        let mut kbuf = Vec::new();
        kbuf.resize(chunk, 0);
        let n = match vfs::fd::with_current_io(fd, |handle| {
                  handle.read_at(file_off, &mut kbuf)
              }) {
            Ok(n) => n,
            Err(err) => return UserRet::from_error(vfs_io_at_error_to_errno(err)),
        };
        if n == 0 {
            break;
        }
        gathered.extend_from_slice(&kbuf[..n]);
        file_off = match file_off.checked_add(n as u64) {
            Some(v) => v,
            None => return UserRet::from_error(ErrNo::EINVAL),
        };
        remaining -= n;
    }

    let scattered = match scatter_to_user_iovecs(iov_ptr, iovcnt, &gathered) {
        Ok(n) => n,
        Err(e) => return UserRet::from_error(e),
    };
    log::info!("[sys_preadv] fd={} ret={}/{}",
               fd,
               scattered,
               want);
    UserRet::from_success(scattered)
}

// 本方法代码由AI完成
pub(crate) fn sys_pwritev(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let iov_ptr = args.arg(1);
    let iovcnt = args.arg(2);
    let offset = match offset_from_arg(args.arg(3)) {
        Ok(v) => v,
        Err(e) => return UserRet::from_error(e),
    };

    let data = match gather_user_iovecs(iov_ptr, iovcnt) {
        Ok(v) => v,
        Err(e) => return UserRet::from_error(e),
    };
    if data.is_empty() {
        return UserRet::from_success(0);
    }
    match vfs::fd::with_current_io(fd, |handle| {
              handle.write_at(offset, &data)
          }) {
        Ok(n) => UserRet::from_success(n),
        Err(err) => UserRet::from_error(vfs_io_at_error_to_errno(err)),
    }
}

pub(crate) fn sys_lseek(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let offset = args.arg(1) as i64;
    let whence = args.arg(2);

    let whence = match whence as u32 {
        SEEK_SET => VfsSeekWhence::Set,
        SEEK_CUR => VfsSeekWhence::Cur,
        SEEK_END => VfsSeekWhence::End,
        _ => return UserRet::from_error(ErrNo::EINVAL),
    };

    let result = loop {
        match vfs::fd::with_current_io(fd, |handle| handle.seek(offset, whence)) {
            Err(VfsError::Busy) => task::yield_now(),
            result => break result,
        }
    };
    match result {
        Ok(pos) => UserRet::from_success(pos as usize),
        Err(e) => {
            let errno = match vfs_error_to_errno(e) {
                ErrNo::EINVAL => ErrNo::ESPIPE,
                other => other,
            };
            UserRet::from_error(errno)
        }
    }
}

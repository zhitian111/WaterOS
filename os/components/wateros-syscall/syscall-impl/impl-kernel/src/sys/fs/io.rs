//! 文件 I/O 操作：`read`/`readv`、`write`/`writev`、`pread64`/`pwrite64`/`preadv`/`pwritev`/`lseek`。

extern crate alloc;
use crate::fallible_buf::{try_kbuf, SYSCALL_IO_MAX};
use crate::socket_block::socket_blocking_tick;
use crate::socket_fd;
use crate::user_copy::{
    copy_from_user, copy_from_user_struct, copy_to_user_progress, UserWriteProgress,
};
use crate::vfs_util::{vfs_error_to_errno, vfs_io_at_error_to_errno};
use alloc::boxed::Box;
use alloc::vec::Vec;
use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use core::sync::atomic::{AtomicU64, Ordering};
use network::{stack, SocketKind, SocketRef, SocketSendError};
use vfs::api::{VfsCopyProgress, VfsError, VfsReadFinish, VfsReadLease, VfsSeekWhence};

const MAX_IO : usize = 0x7FFFF000;
const IO_CHUNK : usize = 64 * 1024;
const IOV_MAX : usize = 1024;
const TCP_BULK_WRITE_YIELD_THRESHOLD : usize = 4096;
const TCP_MSS_BYTES : usize = 1460;
const TCP_LOOPBACK_POLL_ROUNDS : usize = 4;
const UDP_SMALL_WRITE_YIELD_THRESHOLD : usize = 256;
const UDP_BULK_WRITE_YIELD_INTERVAL : u64 = 4;

// WaterOS 尚未实现 Linux `rwf_t` 的 per-call direct-I/O/elevator 语义。
// Linux 对当前文件不支持的 RWF 位（也包括未知位组合）返回 EOPNOTSUPP；
// 不能返回 EINVAL，否则 LTP 和依赖回退逻辑的 libc 会误判参数本身非法。

static UDP_BULK_WRITE_COUNT : AtomicU64 = AtomicU64::new(0);

const SEEK_SET : u32 = 0;
const SEEK_CUR : u32 = 1;
const SEEK_END : u32 = 2;

/// 用户态 iovec 结构（与 Linux `struct iovec` 布局一致）。
#[repr(C)]
#[derive(Clone, Copy)]
struct UserIoVec {
    base : usize,
    len : usize,
}

#[derive(Clone, Copy)]
struct ImportedIoVec {
    base : usize,
    len : usize,
}

struct ImportedIoVecs {
    entries : Vec<ImportedIoVec>,
    total_len : usize,
}


pub(crate) fn sys_read(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let ptr = args.arg(1);
    let len = args.arg(2);
    if let Err(err) = validate_read_fd(fd) {
        return UserRet::from_error(err);
    }
    if let Err(error) = check_tty_foreground(fd, false) {
        return UserRet::from_error(error);
    }
    if len == 0 {
        return UserRet::from_success(0);
    }
    if ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let transfer_len = read_transfer_len(len);
    let lease = match acquire_read_lease(fd, transfer_len) {
        Ok(lease) => lease,
        Err(error) => return UserRet::from_error(error),
    };
    let progress = copy_to_user_progress(ptr, lease.bytes());
    finish_scattered_read(lease, progress)
}

fn acquire_read_lease(fd : usize, transfer_len : usize) -> Result<Box<dyn VfsReadLease>, ErrNo> {
    let socket_wait = socket_fd::lookup(fd).map(|_| {
                                               (socket_fd::is_nonblocking(fd),
                                                task::current_task_id().unwrap_or(0))
                                           });
    let lease = loop {
        if socket_wait.is_some() {
            drive_network_stack();
        }
        let prepared = match vfs::fd::prepare_current_read(fd, transfer_len) {
            Ok(prepared) => prepared,
            Err(VfsError::Busy) => {
                wait_for_read_retry(socket_wait)?;
                continue;
            }
            Err(error) => return Err(vfs_error_to_errno(error)),
        };
        match prepared.acquire() {
            Ok(lease) => break lease,
            Err(VfsError::Busy) => wait_for_read_retry(socket_wait)?,
            Err(error) => return Err(vfs_error_to_errno(error)),
        }
    };
    Ok(lease)
}

fn wait_for_read_retry(socket_wait : Option<(bool, usize)>) -> Result<(), ErrNo> {
    if let Some((nonblocking, task_id)) = socket_wait {
        socket_blocking_tick(nonblocking, task_id)
    } else {
        task::yield_now();
        Ok(())
    }
}

fn finish_scattered_read(lease : Box<dyn VfsReadLease>, progress : UserWriteProgress) -> UserRet {
    let finish = lease.finish(VfsCopyProgress { copied : progress.copied,
                                                complete : progress.error
                                                                   .is_none() })
                      .map_err(vfs_error_to_errno);
    match finish {
        Ok(VfsReadFinish::Bytes(copied))
            if copied > 0 ||
               progress.error
                       .is_none() =>
        {
            UserRet::from_success(copied)
        }
        Ok(VfsReadFinish::Bytes(_)) | Ok(VfsReadFinish::Fault) => {
            UserRet::from_error(progress.error
                                        .unwrap_or(ErrNo::EFAULT))
        }
        Err(error) => UserRet::from_error(error),
    }
}

fn validate_read_fd(fd : usize) -> Result<(), ErrNo> {
    if socket_fd::lookup(fd).is_some() {
        return Ok(());
    }
    if vfs::fd::is_path_only_fd(fd).map_err(vfs_error_to_errno)? {
        return Err(ErrNo::EBADF);
    }
    vfs::fd::with_current_io(fd, |handle| {
        handle.validate_read_access()
    }).map_err(vfs_error_to_errno)
}

/// 内核缓冲区有独立上限；大 `count` 通过合法短读分批完成，不能返回 `EINVAL`。
fn read_transfer_len(requested : usize) -> usize {
    requested.min(MAX_IO)
             .min(SYSCALL_IO_MAX)
}

fn import_iovecs(iov_ptr : usize, iovcnt : usize) -> Result<ImportedIoVecs, ErrNo> {
    if iovcnt > IOV_MAX {
        return Err(ErrNo::EINVAL);
    }
    if iovcnt > 0 && iov_ptr == 0 {
        return Err(ErrNo::EFAULT);
    }
    let mut entries = Vec::new();
    entries.try_reserve_exact(iovcnt)
           .map_err(|_| ErrNo::ENOMEM)?;
    let iov_size = core::mem::size_of::<UserIoVec>();
    let mut total_len = 0usize;
    for index in 0..iovcnt {
        let address = index.checked_mul(iov_size)
                           .and_then(|offset| iov_ptr.checked_add(offset))
                           .ok_or(ErrNo::EFAULT)?;
        let iov = copy_from_user_struct::<UserIoVec>(address)?;
        if iov.len > 0 && iov.base == 0 {
            return Err(ErrNo::EFAULT);
        }
        total_len = total_len.checked_add(iov.len)
                             .ok_or(ErrNo::EINVAL)?;
        if total_len > isize::MAX as usize {
            return Err(ErrNo::EINVAL);
        }
        entries.push(ImportedIoVec { base : iov.base,
                                     len : iov.len });
    }
    Ok(ImportedIoVecs { entries, total_len })
}

struct IovScatterCursor<'a> {
    entries : &'a [ImportedIoVec],
    index : usize,
    offset : usize,
    copied : usize,
}

impl<'a> IovScatterCursor<'a> {
    fn new(entries : &'a [ImportedIoVec]) -> Self {
        Self { entries,
               index : 0,
               offset : 0,
               copied : 0 }
    }

    fn write(&mut self, data : &[u8]) -> UserWriteProgress {
        let start = self.copied;
        let mut source_offset = 0usize;
        while source_offset < data.len() {
            while self.index < self.entries.len() && self.offset == self.entries[self.index].len {
                self.index += 1;
                self.offset = 0;
            }
            let Some(iov) = self.entries
                                .get(self.index)
                                .copied()
            else {
                return UserWriteProgress { copied : self.copied - start,
                                           error : Some(ErrNo::EFAULT) };
            };
            let chunk = (iov.len - self.offset).min(data.len() - source_offset);
            let destination = match iov.base
                                       .checked_add(self.offset)
            {
                Some(destination) => destination,
                None => {
                    return UserWriteProgress { copied : self.copied - start,
                                               error : Some(ErrNo::EFAULT) };
                }
            };
            let progress = copy_to_user_progress(destination,
                                                 &data[source_offset..source_offset + chunk]);
            source_offset += progress.copied;
            self.offset += progress.copied;
            self.copied += progress.copied;
            if progress.error
                       .is_some()
            {
                return UserWriteProgress { copied : self.copied - start,
                                           error : Some(ErrNo::EFAULT) };
            }
            if progress.copied != chunk {
                return UserWriteProgress { copied : self.copied - start,
                                           error : Some(ErrNo::EFAULT) };
            }
        }
        UserWriteProgress { copied : self.copied - start,
                            error : None }
    }
}

fn scatter_progress(iovecs : &[ImportedIoVec], data : &[u8]) -> UserWriteProgress {
    IovScatterCursor::new(iovecs).write(data)
}

// 本方法代码由AI完成
pub(crate) fn sys_readv(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let iov_ptr = args.arg(1);
    let iovcnt = args.arg(2);
    if let Err(error) = validate_read_fd(fd) {
        return UserRet::from_error(error);
    }
    if let Err(error) = check_tty_foreground(fd, false) {
        return UserRet::from_error(error);
    }
    let iovecs = match import_iovecs(iov_ptr, iovcnt) {
        Ok(iovecs) => iovecs,
        Err(error) => return UserRet::from_error(error),
    };
    if iovecs.total_len == 0 {
        return UserRet::from_success(0);
    }
    let lease = match acquire_read_lease(fd, read_transfer_len(iovecs.total_len)) {
        Ok(lease) => lease,
        Err(error) => return UserRet::from_error(error),
    };
    let progress = scatter_progress(&iovecs.entries, lease.bytes());
    finish_scattered_read(lease, progress)
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
        assert_eq!(read_transfer_len(SYSCALL_IO_MAX + 1),
                   SYSCALL_IO_MAX);
        assert_eq!(read_transfer_len(usize::MAX),
                   SYSCALL_IO_MAX);
    }

    #[test]
    fn write_transfer_len_preserves_small_requests() {
        assert_eq!(write_transfer_len(4096), 4096);
    }

    #[test]
    fn write_transfer_len_turns_large_requests_into_short_writes() {
        assert_eq!(write_transfer_len(SYSCALL_IO_MAX + 1),
                   SYSCALL_IO_MAX);
        assert_eq!(write_transfer_len(usize::MAX),
                   SYSCALL_IO_MAX);
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

fn flush_segmented_loopback_send(socket : &SocketRef, sent : usize) {
    if sent <= TCP_MSS_BYTES ||
       !socket.peer_is_loopback()
              .unwrap_or(false)
    {
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
    if let Err(error) = validate_write_fd(fd) {
        return UserRet::from_error(error);
    }
    if let Err(error) = check_tty_foreground(fd, true) {
        return UserRet::from_error(error);
    }
    if len == 0 {
        return UserRet::from_success(0);
    }
    if ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let transfer_len = write_transfer_len(len);
    if let Err(error) = check_fsize_rlimit(fd, transfer_len) {
        return UserRet::from_error(error);
    }
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

/// Linux `RLIMIT_FSIZE`：写后文件结束偏移超过软限制时，若 `SIGXFSZ`
/// 被忽略/捕获则返回 `EFBIG`（LTP llseek01）。默认限制为无穷大时该
/// 检查零开销跳过。
fn check_fsize_rlimit(fd : usize, write_len : usize) -> Result<(), ErrNo> {
    let Some(pid) = task::current_process_task_snapshot().map(|snapshot| snapshot.pid) else {
        return Ok(());
    };
    let Some(limit) = task::process_resource_limit(pid, super::truncate::RLIMIT_FSIZE) else {
        return Ok(());
    };
    if limit.cur == u64::MAX {
        return Ok(());
    }
    // 当前文件偏移（`SEEK_CUR` + 0 不改动位置）；pipe/socket 等不支持
    // seek，视为无文件大小限制。
    let offset = match vfs::fd::with_current_io_detached(fd, |handle| {
              handle.seek(0, VfsSeekWhence::Cur)
          }) {
        Ok(offset) => offset,
        Err(_) => return Ok(()),
    };
    if offset.saturating_add(write_len as u64) > limit.cur {
        return Err(ErrNo::EFBIG);
    }
    Ok(())
}

// 本方法代码由AI完成
pub(crate) fn sys_writev(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let iov_ptr = args.arg(1);
    let iovcnt = args.arg(2);
    if let Err(error) = validate_write_fd(fd) {
        return UserRet::from_error(error);
    }
    if let Err(error) = check_tty_foreground(fd, true) {
        return UserRet::from_error(error);
    }
    let iovecs = match import_iovecs(iov_ptr, iovcnt) {
        Ok(iovecs) => iovecs,
        Err(error) => return UserRet::from_error(error),
    };
    let out = match gather_imported_iovecs(&iovecs) {
        Ok(out) => out,
        Err(error) => return UserRet::from_error(error),
    };

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
        return match socket.kind() {
            Ok(SocketKind::Tcp) => write_tcp_socket_blocking(fd, buf),
            Ok(SocketKind::Udp | SocketKind::Icmp) => write_udp_socket_blocking(fd, buf),
            Err(_) => Err(ErrNo::ENOTSOCK),
        };
    }
    if vfs::fd::is_path_only_fd(fd).map_err(vfs_error_to_errno)? {
        return Err(ErrNo::EBADF);
    }
    loop {
        // pipe/socketpair 写满时会进入等待队列；使用独立临时句柄，避免睡眠时
        // 占住共享 fd 槽锁并与同进程的读/poll 线程形成单核死锁。
        match vfs::fd::with_current_io_detached(fd, |handle| handle.write(buf)) {
            Err(VfsError::Busy) => task::yield_now(),
            Ok(written) => {
                dispatch_pty_control_events(fd);
                if written != 0 {
                    super::inotify::notify_fd_modify(fd);
                }
                return Ok(written);
            }
            Err(error) => return Err(vfs_error_to_errno(error)),
        }
    }
}

/// master 输入产生的 Ctrl-C/Ctrl-Z 等事件在 PTY 锁外投递。
fn dispatch_pty_control_events(fd : usize) {
    let Ok(Some(endpoint)) = vfs::fd::current_pty_endpoint(fd) else {
        return;
    };
    for event in tty::take_control_events(endpoint.id()) {
        crate::sys::ipc::signal::send_kernel_signal_to_process_group(
            task::ProcessId::from_raw(event.process_group), event.signal);
    }
}

fn validate_write_fd(fd : usize) -> Result<(), ErrNo> {
    if socket_fd::lookup(fd).is_some() {
        return Ok(());
    }
    if vfs::fd::is_path_only_fd(fd).map_err(vfs_error_to_errno)? {
        return Err(ErrNo::EBADF);
    }
    vfs::fd::with_current_io(fd, |handle| {
        const O_ACCMODE : u32 = 3;
        const O_RDONLY : u32 = 0;
        if handle.open_accmode() & O_ACCMODE == O_RDONLY {
            Err(VfsError::BadFd)
        } else {
            Ok(())
        }
    }).map_err(vfs_error_to_errno)
}

/// Enforce controlling-terminal foreground process-group rules before user
/// memory is copied. Terminal-generated signals use the kernel delivery path,
/// then the interrupted syscall returns `EINTR`; the trap layer already knows
/// how to restart read/write when the installed action has `SA_RESTART`.
fn check_tty_foreground(fd : usize, writing : bool) -> Result<(), ErrNo> {
    if !vfs::fd::current_fd_is_tty_char(fd).unwrap_or(false) {
        return Ok(());
    }
    let pty = vfs::fd::current_pty_endpoint(fd).ok()
                                               .flatten();
    // PTY master 是终端模拟器的数据端，不是受前台进程组限制的控制终端端点。
    if pty.as_ref()
          .is_some_and(|endpoint| endpoint.endpoint() == tty::TerminalEndpoint::PtyMaster)
    {
        return Ok(());
    }
    let stops_background = pty.as_ref()
                              .map_or_else(tty::output_stops_background,
                                           tty::PtyEndpointHandle::output_stops_background);
    if writing && !stops_background {
        return Ok(());
    }
    let foreground = pty.as_ref()
                        .map_or_else(tty::foreground_pgid,
                                     tty::PtyEndpointHandle::foreground_pgid);
    if foreground == 0 {
        return Ok(());
    }
    let Some(process) = task::current_process_snapshot() else {
        return Ok(());
    };
    let controlling_sid = pty.as_ref()
                             .map_or_else(tty::controlling_sid,
                                          tty::PtyEndpointHandle::controlling_sid);
    // 只有把该 terminal 作为控制终端的会话才受前后台作业控制约束。
    // 通过 fd 传递拿到别的会话 PTY 时，不能误向当前进程组发送 SIGTTIN/SIGTTOU。
    if controlling_sid == 0 || controlling_sid != process.sid.raw() {
        return Ok(());
    }
    if process.pgid.raw() == foreground {
        return Ok(());
    }
    let signal = if writing {
        ipc::signal::SIGTTOU
    } else {
        ipc::signal::SIGTTIN
    };
    crate::sys::ipc::signal::send_kernel_signal_to_process_group(process.pgid, signal);
    Err(ErrNo::EINTR)
}

fn write_tcp_socket_blocking(fd : usize, buf : &[u8]) -> Result<usize, ErrNo> {
    let nonblocking = socket_fd::is_nonblocking(fd);
    let task_id = task::current_task_id().unwrap_or(0);
    loop {
        let socket = socket_fd::lookup(fd).ok_or(ErrNo::ENOTSOCK)?;
        drive_network_stack();
        let snapshot = socket.poll_snapshot()
                             .map_err(|_| ErrNo::ENOTSOCK)?;
        if snapshot.may_send && snapshot.send_capacity > 0 {
            let send_len = buf.len()
                              .min(snapshot.send_capacity);
            match socket.send(&buf[..send_len]) {
                Ok(n) if n > 0 => {
                    flush_segmented_loopback_send(&socket, n);
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
        if !snapshot.is_connected {
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
        match socket.send(buf) {
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
            Err(SocketSendError::WouldBlock) => {
                socket_blocking_tick(nonblocking, task_id)?;
            }
            Err(err) => {
                return Err(crate::sys::net::sendto::socket_send_error_to_errno(err));
            }
        }
    }
}

fn offset_from_arg(raw : usize) -> Result<u64, ErrNo> { offset_from_bits(raw as u64) }

/// Linux asm-generic 的 `preadv*`/`pwritev*` 把 `loff_t` 拆成两个
/// 32 位槽位，低位在前、高位在后。显式截成 `u32` 也可正确处理调用方
/// 对负值参数做了机器字宽符号扩展的情况。
fn split_offset_bits(low : usize, high : usize) -> u64 {
    u64::from(low as u32) | (u64::from(high as u32) << 32)
}

fn split_offset_from_args(low : usize, high : usize) -> Result<u64, ErrNo> {
    offset_from_bits(split_offset_bits(low, high))
}

fn offset_from_bits(raw : u64) -> Result<u64, ErrNo> {
    let off = raw as i64;
    if off < 0 {
        return Err(ErrNo::EINVAL);
    }
    Ok(off as u64)
}

fn gather_imported_iovecs(iovecs : &ImportedIoVecs) -> Result<Vec<u8>, ErrNo> {
    let mut out = Vec::new();
    for iov in &iovecs.entries {
        if iov.len == 0 {
            continue;
        }
        let remaining = SYSCALL_IO_MAX - out.len();
        if remaining == 0 {
            break;
        }
        let segment_len = iov.len
                             .min(remaining);
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

fn validate_pread_fd(fd : usize) -> Result<(), ErrNo> {
    if socket_fd::lookup(fd).is_some() {
        return Err(ErrNo::ESPIPE);
    }
    if vfs::fd::is_path_only_fd(fd).map_err(vfs_error_to_errno)? {
        return Err(ErrNo::EBADF);
    }
    vfs::fd::with_current_io(fd, |handle| {
        handle.validate_read_access()?;
        let mut empty = [];
        handle.read_at(0, &mut empty)
              .map(|_| ())
    }).map_err(vfs_io_at_error_to_errno)
}

// 本方法代码由AI完成
pub(crate) fn sys_pread64(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let ptr = args.arg(1);
    let len = args.arg(2);
    if let Err(error) = validate_pread_fd(fd) {
        return UserRet::from_error(error);
    }
    let offset = match offset_from_arg(args.arg(3)) {
        Ok(offset) => offset,
        Err(error) => return UserRet::from_error(error),
    };
    if len == 0 {
        return UserRet::from_success(0);
    }
    if ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let transfer_len = read_transfer_len(len);
    let mut kbuf = match try_kbuf(transfer_len, SYSCALL_IO_MAX) {
        Ok(kbuf) => kbuf,
        Err(error) => return UserRet::from_error(error),
    };
    let n = match vfs::fd::with_current_io(fd, |handle| {
              handle.read_at(offset, &mut kbuf)
          }) {
        Ok(n) => n,
        Err(err) => return UserRet::from_error(vfs_io_at_error_to_errno(err)),
    };
    if n == 0 {
        return UserRet::from_success(0);
    }
    let progress = copy_to_user_progress(ptr, &kbuf[..n]);
    if progress.copied > 0 ||
       progress.error
               .is_none()
    {
        UserRet::from_success(progress.copied)
    } else {
        UserRet::from_error(ErrNo::EFAULT)
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_pwrite64(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let ptr = args.arg(1);
    let len = args.arg(2);
    const O_APPEND : u32 = 0o2000;
    if let Err(error) = validate_write_fd(fd) {
        return UserRet::from_error(error);
    }
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
    let append = match vfs::fd::with_current_io(fd, |handle| {
              Ok(handle.open_status_flags() & O_APPEND != 0)
          }) {
        Ok(value) => value,
        Err(err) => return UserRet::from_error(vfs_io_at_error_to_errno(err)),
    };
    let write_offset = if append {
        match vfs::fd::with_current_io(fd, |handle| {
                  handle.metadata()
                        .map(|meta| meta.size)
              }) {
            Ok(size) => size,
            Err(err) => return UserRet::from_error(vfs_io_at_error_to_errno(err)),
        }
    } else {
        offset
    };
    match vfs::fd::with_current_io(fd, |handle| {
              handle.write_at(write_offset, &kbuf)
          }) {
        Ok(n) => {
            if n != 0 {
                super::inotify::notify_fd_modify(fd);
            }
            UserRet::from_success(n)
        }
        Err(err) => UserRet::from_error(vfs_io_at_error_to_errno(err)),
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_preadv(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let iov_ptr = args.arg(1);
    let iovcnt = args.arg(2);
    let offset = match split_offset_from_args(args.arg(3), args.arg(4)) {
        Ok(offset) => offset,
        Err(error) => return UserRet::from_error(error),
    };
    preadv_at(fd, iov_ptr, iovcnt, offset)
}

/// 执行已经完成 Linux 分槽 ABI 解码的向量定位读。
fn preadv_at(fd : usize, iov_ptr : usize, iovcnt : usize, offset : u64) -> UserRet {
    if let Err(error) = validate_pread_fd(fd) {
        return UserRet::from_error(error);
    }
    let iovecs = match import_iovecs(iov_ptr, iovcnt) {
        Ok(iovecs) => iovecs,
        Err(error) => return UserRet::from_error(error),
    };
    if iovecs.total_len == 0 {
        return UserRet::from_success(0);
    }
    let want = read_transfer_len(iovecs.total_len);
    let mut kbuf = match try_kbuf(want.min(IO_CHUNK), IO_CHUNK) {
        Ok(kbuf) => kbuf,
        Err(error) => return UserRet::from_error(error),
    };
    let mut file_off = offset;
    let mut remaining = want;
    let mut cursor = IovScatterCursor::new(&iovecs.entries);
    while remaining > 0 {
        let chunk = remaining.min(kbuf.len());
        let n = match vfs::fd::with_current_io(fd, |handle| {
                  handle.read_at(file_off, &mut kbuf[..chunk])
              }) {
            Ok(n) => n,
            Err(error) => {
                return if cursor.copied > 0 {
                    UserRet::from_success(cursor.copied)
                } else {
                    UserRet::from_error(vfs_io_at_error_to_errno(error))
                };
            }
        };
        if n == 0 {
            break;
        }
        let progress = cursor.write(&kbuf[..n]);
        if let Some(error) = progress.error {
            return if cursor.copied > 0 {
                UserRet::from_success(cursor.copied)
            } else {
                UserRet::from_error(error)
            };
        }
        file_off = match file_off.checked_add(n as u64) {
            Some(v) => v,
            None => {
                return if cursor.copied > 0 {
                    UserRet::from_success(cursor.copied)
                } else {
                    UserRet::from_error(ErrNo::EINVAL)
                };
            }
        };
        remaining -= n;
        if n < chunk {
            break;
        }
    }
    UserRet::from_success(cursor.copied)
}

/// `preadv2(2)`：零 flags 复用完整的 `preadv` 路径；offset=-1 按 Linux
/// 语义使用并推进文件描述符当前位置。高级 RWF 能力尚无后端时返回
/// `EOPNOTSUPP`，未知位返回 `EINVAL`。
pub(crate) fn sys_preadv2(args : SyscallArgs) -> UserRet {
    // Linux 的原始 ABI 是
    // (fd, iov, iovcnt, offset_low, offset_high, flags)，即便在 64 位
    // asm-generic 架构上 offset 也占两个参数槽。不能把 offset_high 当 flags。
    let flags = args.arg(5);
    if flags != 0 {
        return UserRet::from_error(ErrNo::EOPNOTSUPP);
    }
    let raw_offset = split_offset_bits(args.arg(3), args.arg(4));
    if raw_offset == u64::MAX {
        return sys_readv(args);
    }
    let offset = match offset_from_bits(raw_offset) {
        Ok(offset) => offset,
        Err(error) => return UserRet::from_error(error),
    };
    preadv_at(args.arg(0),
              args.arg(1),
              args.arg(2),
              offset)
}

// 本方法代码由AI完成
pub(crate) fn sys_pwritev(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let iov_ptr = args.arg(1);
    let iovcnt = args.arg(2);
    let offset = match split_offset_from_args(args.arg(3), args.arg(4)) {
        Ok(offset) => offset,
        Err(error) => return UserRet::from_error(error),
    };
    pwritev_at(fd, iov_ptr, iovcnt, offset)
}

/// 执行已经完成 Linux 分槽 ABI 解码的向量定位写。
fn pwritev_at(fd : usize, iov_ptr : usize, iovcnt : usize, offset : u64) -> UserRet {
    if let Err(error) = validate_write_fd(fd) {
        return UserRet::from_error(error);
    }
    let iovecs = match import_iovecs(iov_ptr, iovcnt) {
        Ok(iovecs) => iovecs,
        Err(error) => return UserRet::from_error(error),
    };
    let data = match gather_imported_iovecs(&iovecs) {
        Ok(v) => v,
        Err(e) => return UserRet::from_error(e),
    };
    if data.is_empty() {
        return UserRet::from_success(0);
    }
    match vfs::fd::with_current_io(fd, |handle| {
              handle.write_at(offset, &data)
          }) {
        Ok(n) => {
            if n != 0 {
                super::inotify::notify_fd_modify(fd);
            }
            UserRet::from_success(n)
        }
        Err(err) => UserRet::from_error(vfs_io_at_error_to_errno(err)),
    }
}

/// `pwritev2(2)` 的基础兼容实现，规则与 [`sys_preadv2`] 对称。
pub(crate) fn sys_pwritev2(args : SyscallArgs) -> UserRet {
    let flags = args.arg(5);
    if flags != 0 {
        return UserRet::from_error(ErrNo::EOPNOTSUPP);
    }
    let raw_offset = split_offset_bits(args.arg(3), args.arg(4));
    if raw_offset == u64::MAX {
        return sys_writev(args);
    }
    let offset = match offset_from_bits(raw_offset) {
        Ok(offset) => offset,
        Err(error) => return UserRet::from_error(error),
    };
    pwritev_at(args.arg(0),
               args.arg(1),
               args.arg(2),
               offset)
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
            // Linux 语义：对不可 seek 的句柄（pipe/socket/fifo、解压流等）
            // 调用 lseek 必须返回 ESPIPE，调用方（如 dpkg 跳过 tar padding）
            // 依赖该 errno 回退为流式读取；若返回 EOPNOTSUPP 会被误判为硬错误。
            let errno = match e {
                VfsError::Unsupported => ErrNo::ESPIPE,
                other => match vfs_error_to_errno(other) {
                    ErrNo::EINVAL => ErrNo::ESPIPE,
                    errno => errno,
                },
            };
            UserRet::from_error(errno)
        }
    }
}

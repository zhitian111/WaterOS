//! `read(2)`：支持 pipe 读端；stdin 暂未接真实输入。

//! 本模块代码由AI完成
extern crate alloc;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use driver::network::stack;

use crate::fallible_buf::{try_kbuf, SYSCALL_IO_MAX};
use crate::socket_block::socket_blocking_tick;
use crate::socket_fd;
use crate::user_copy::{copy_from_user_struct, copy_to_user};
use crate::vfs_util::vfs_error_to_errno;

const SMALL_READ_BUF_SIZE : usize = 256;

#[repr(C)]
#[derive(Clone, Copy)]
struct UserIoVec {
    base : usize,
    len : usize,
}

fn finish_read(_fd : usize, ptr : usize, buf : &[u8], n : usize) -> UserRet {
    if n == 0 {
        return UserRet::from_success(0);
    }
    match copy_to_user(ptr, &buf[..n]) {
        Ok(written) if written == n => UserRet::from_success(n),
        _ => UserRet::from_error(ErrNo::EFAULT),
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_read(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let ptr = args.arg(1);
    let len = args.arg(2);
    if len == 0 {
        return UserRet::from_success(0);
    }
    if ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if len > SYSCALL_IO_MAX {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if len <= SMALL_READ_BUF_SIZE {
        let mut kbuf = [0u8; SMALL_READ_BUF_SIZE];
        let n = match read_fd(fd, &mut kbuf[..len]) {
            Ok(n) => n,
            Err(err) => return UserRet::from_error(err),
        };
        return finish_read(fd, ptr, &kbuf[..len], n);
    }
    let mut kbuf = match try_kbuf(len, SYSCALL_IO_MAX) {
        Ok(buf) => buf,
        Err(err) => return UserRet::from_error(err),
    };
    let n = match read_fd(fd, &mut kbuf) {
        Ok(n) => n,
        Err(err) => return UserRet::from_error(err),
    };
    finish_read(fd, ptr, &kbuf, n)
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

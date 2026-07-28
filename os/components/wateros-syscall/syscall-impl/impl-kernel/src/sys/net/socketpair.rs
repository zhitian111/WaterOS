//! `socketpair(2)`：创建一对已连接的 AF_UNIX stream-compatible socket fd。

//! 本模块代码由AI完成
use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::user_copy::copy_to_user;
use crate::vfs_util::vfs_error_to_errno;

const AF_UNIX: usize = 1;
const SOCK_STREAM: usize = 1;
const SOCK_DGRAM: usize = 2;
const SOCK_SEQPACKET: usize = 5;
const SOCK_NONBLOCK: usize = 0o4000;
const SOCK_CLOEXEC: usize = 0o2000000;
const FD_CLOEXEC: usize = 1;

// 本方法代码由AI完成
pub(crate) fn sys_socketpair(args: SyscallArgs) -> UserRet {
    let domain = args.arg(0);
    let mut typ = args.arg(1);
    let protocol = args.arg(2);
    let sv_ptr = args.arg(3);

    if sv_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if domain != AF_UNIX {
        return UserRet::from_error(ErrNo::EAFNOSUPPORT);
    }
    if protocol != 0 {
        return UserRet::from_error(ErrNo::EPROTONOSUPPORT);
    }

    let cloexec = typ & SOCK_CLOEXEC != 0;
    let nonblocking = typ & SOCK_NONBLOCK != 0;
    typ &= !(SOCK_NONBLOCK | SOCK_CLOEXEC);

    if typ != SOCK_STREAM && typ != SOCK_SEQPACKET {
        if typ == SOCK_DGRAM {
            return UserRet::from_error(ErrNo::EPROTONOSUPPORT);
        }
        return UserRet::from_error(ErrNo::EPROTONOSUPPORT);
    }

    let task_id = match vfs::fd::current_task_id() {
        Ok(task_id) => task_id,
        Err(err) => return UserRet::from_error(vfs_error_to_errno(err)),
    };

    let ((end0, sock0), (end1, sock1)) =
        crate::unix_sock::alloc_unix_stream_pair(nonblocking);
    let (fd0, fd1) =
        match vfs::fd::with_registry(|reg| -> vfs::VfsResult<(usize, usize)> {
            let fd0 = reg.alloc_fd_for_task(task_id, end0)?;
            let fd1 = match reg.alloc_fd_for_task(task_id, end1) {
                Ok(fd) => fd,
                Err(err) => {
                    let _ = reg.close_fd_for_task(task_id, fd0);
                    return Err(err);
                }
            };
            if cloexec {
                let _ = reg.set_fd_flags(task_id, fd0, FD_CLOEXEC);
                let _ = reg.set_fd_flags(task_id, fd1, FD_CLOEXEC);
            }
            Ok((fd0, fd1))
        }) {
            Ok(fds) => fds,
            Err(err) => return UserRet::from_error(vfs_error_to_errno(err)),
        };
    crate::unix_sock::register(fd0, sock0);
    crate::unix_sock::register(fd1, sock1);

    let fds = [fd0 as i32, fd1 as i32];
    match copy_to_user(sv_ptr, unsafe {
        core::slice::from_raw_parts(fds.as_ptr() as *const u8, core::mem::size_of_val(&fds))
    }) {
        Ok(n) if n == core::mem::size_of_val(&fds) => UserRet::from_success(0),
        _ => {
            crate::unix_sock::unregister(task_id, fd0);
            crate::unix_sock::unregister(task_id, fd1);
            let _ = vfs::fd::close_fd(fd0);
            let _ = vfs::fd::close_fd(fd1);
            UserRet::from_error(ErrNo::EFAULT)
        }
    }
}

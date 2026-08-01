//! `socketpair(2)`：创建一对已连接的 AF_UNIX stream-compatible socket fd。

//! 本模块代码由AI完成
use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;

use crate::user_copy::copy_to_user;
use crate::vfs_util::vfs_error_to_errno;

const AF_UNIX: usize = 1;
const AF_INET: usize = 2;
const SOCK_STREAM: usize = 1;
const SOCK_DGRAM: usize = 2;
const SOCK_RAW: usize = 3;
const SOCK_SEQPACKET: usize = 5;
const SOCK_NONBLOCK: usize = 0o4000;
const SOCK_CLOEXEC: usize = 0o2000000;
const FD_CLOEXEC: usize = 1;
const IPPROTO_TCP: usize = 6;
const IPPROTO_UDP: usize = 17;

// 本方法代码由AI完成
pub(crate) fn sys_socketpair(args: SyscallArgs) -> UserRet {
    let domain = args.arg(0);
    let mut typ = args.arg(1);
    let protocol = args.arg(2);
    let sv_ptr = args.arg(3);

    let cloexec = typ & SOCK_CLOEXEC != 0;
    let nonblocking = typ & SOCK_NONBLOCK != 0;
    typ &= !(SOCK_NONBLOCK | SOCK_CLOEXEC);

    if !matches!(typ, SOCK_STREAM | SOCK_DGRAM | SOCK_RAW | SOCK_SEQPACKET) {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if domain != AF_UNIX {
        if domain == AF_INET {
            let protocol_matches = match typ {
                SOCK_STREAM => protocol == 0 || protocol == IPPROTO_TCP,
                SOCK_DGRAM => protocol == 0 || protocol == IPPROTO_UDP,
                _ => false,
            };
            return if protocol_matches {
                UserRet::from_error(ErrNo::EOPNOTSUPP)
            } else {
                UserRet::from_error(ErrNo::EPROTONOSUPPORT)
            };
        }
        return UserRet::from_error(ErrNo::EAFNOSUPPORT);
    }
    if typ == SOCK_RAW || protocol != 0 {
        return UserRet::from_error(ErrNo::EPROTONOSUPPORT);
    }
    if sv_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let task_id = match vfs::fd::current_task_id() {
        Ok(task_id) => task_id,
        Err(err) => return UserRet::from_error(vfs_error_to_errno(err)),
    };

    let ((end0, sock0), (end1, sock1)) = if typ == SOCK_DGRAM {
        crate::unix_sock::alloc_unix_dgram_pair(nonblocking)
    } else {
        crate::unix_sock::alloc_unix_stream_pair(nonblocking)
    };
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

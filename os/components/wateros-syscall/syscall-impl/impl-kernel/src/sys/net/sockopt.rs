//! `setsockopt(2)` / `getsockopt(2)` — 极简存根。
//! 本模块代码由AI完成

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use network::NetworkError;

use crate::fallible_buf::{try_kbuf, SYSCALL_SOCK_IO_MAX};
use crate::socket_fd;
use crate::user_copy::{copy_from_user, copy_from_user_struct, copy_to_user, copy_to_user_struct};

const SOL_IP : usize = 0;
const IPPROTO_TCP : usize = 6;

fn getsockopt_error(error : NetworkError, level : usize) -> ErrNo {
    match error {
        NetworkError::InvalidArgument => ErrNo::EINVAL,
        NetworkError::AddressNotAvailable => ErrNo::EADDRNOTAVAIL,
        NetworkError::WrongSocketType => ErrNo::ENOPROTOOPT,
        NetworkError::Unsupported if matches!(level, SOL_IP | IPPROTO_TCP) => {
            ErrNo::ENOPROTOOPT
        }
        NetworkError::Unsupported => ErrNo::EOPNOTSUPP,
        _ => ErrNo::EOPNOTSUPP,
    }
}

fn setsockopt_error(error : NetworkError) -> ErrNo {
    match error {
        NetworkError::InvalidArgument => ErrNo::EINVAL,
        NetworkError::AddressNotAvailable => ErrNo::EADDRNOTAVAIL,
        NetworkError::Unsupported | NetworkError::WrongSocketType => ErrNo::ENOPROTOOPT,
        _ => ErrNo::EOPNOTSUPP,
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_setsockopt(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let level = args.arg(1);
    let optname = args.arg(2);
    let optval = args.arg(3);
    let optlen = args.arg(4);

    let socket = match socket_fd::lookup_or_errno(fd) {
        Ok(socket) => socket,
        Err(error) => return UserRet::from_error(error),
    };
    if optlen > SYSCALL_SOCK_IO_MAX {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if optlen == 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if optlen > 0 && optval == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let mut kbuf = match try_kbuf(optlen, SYSCALL_SOCK_IO_MAX) {
        Ok(buf) => buf,
        Err(err) => return UserRet::from_error(err),
    };
    if optlen > 0 {
        match copy_from_user(&mut kbuf, optval) {
            Ok(n) if n == optlen => {}
            _ => return UserRet::from_error(ErrNo::EFAULT),
        }
    }

    match socket.set_sockopt(level, optname, &kbuf) {
        Ok(()) => UserRet::from_success(0),
        Err(error) => UserRet::from_error(setsockopt_error(error)),
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_getsockopt(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let level = args.arg(1);
    let optname = args.arg(2);
    let optval = args.arg(3);
    let optlen_ptr = args.arg(4);

    let socket = match socket_fd::lookup_or_errno(fd) {
        Ok(socket) => socket,
        Err(error) => return UserRet::from_error(error),
    };
    if optval == 0 || optlen_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let user_len = match copy_from_user_struct::<u32>(optlen_ptr) {
        Ok(v) => v as usize,
        Err(e) => return UserRet::from_error(e),
    };
    if user_len > SYSCALL_SOCK_IO_MAX {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let value = match socket.get_sockopt(level, optname) {
        Ok(v) => v,
        Err(error) => return UserRet::from_error(getsockopt_error(error, level)),
    };
    let write_len = value.len()
                         .min(user_len);
    if write_len > 0 {
        match copy_to_user(optval, &value[..write_len]) {
            Ok(n) if n == write_len => {}
            _ => return UserRet::from_error(ErrNo::EFAULT),
        }
    }
    match copy_to_user_struct(optlen_ptr, &(write_len as u32)) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}

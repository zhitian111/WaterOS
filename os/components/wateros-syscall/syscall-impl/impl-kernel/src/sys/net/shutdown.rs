//! `shutdown(2)` — 极简存根。
//! 本模块代码由AI完成

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use network::NetworkError;

use crate::socket_fd;

// 本方法代码由AI完成
pub(crate) fn sys_shutdown(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let how = args.arg(1);

    if how > 2 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let socket = match socket_fd::lookup(fd) {
        Some(s) => s,
        None => return UserRet::from_error(ErrNo::ENOTSOCK),
    };

    match socket.shutdown() {
        Ok(()) => UserRet::from_success(0),
        Err(NetworkError::Unsupported) => UserRet::from_error(ErrNo::EOPNOTSUPP),
        Err(_) => UserRet::from_error(ErrNo::EIO),
    }
}

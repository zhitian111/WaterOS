//! `socket(2)`：创建 socket 并分配 fd。

//! 本模块代码由AI完成
use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use network::SocketRef;

const AF_INET : usize = 2;
const AF_UNIX : usize = 1;
const SOCK_STREAM : usize = 1;
const SOCK_DGRAM : usize = 2;
const SOCK_RAW : usize = 3;
const SOCK_NONBLOCK : usize = 0o4000;
const SOCK_CLOEXEC : usize = 0o2000000;
const FD_CLOEXEC : usize = 1;
const IPPROTO_TCP : usize = 6;
const IPPROTO_UDP : usize = 17;

// 本方法代码由AI完成
pub(crate) fn sys_socket(args : SyscallArgs) -> UserRet {
    let domain = args.arg(0);
    let mut typ = args.arg(1);
    let protocol = args.arg(2);

    if typ & !(0xf | SOCK_NONBLOCK | SOCK_CLOEXEC) != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    if domain == AF_UNIX {
        let cloexec = typ & SOCK_CLOEXEC != 0;
        let status_flags = if typ & SOCK_NONBLOCK != 0 {
            SOCK_NONBLOCK
        } else {
            0
        };
        typ &= !(SOCK_NONBLOCK | SOCK_CLOEXEC);
        if !matches!(typ, SOCK_STREAM | SOCK_DGRAM) || protocol != 0 {
            return UserRet::from_error(ErrNo::EINVAL);
        }
        let (io_handle, sock) = match crate::unix_sock::alloc_unix_socket(typ, status_flags) {
            Ok(v) => v,
            Err(e) => return UserRet::from_error(e),
        };
        let fd = match vfs::fd::alloc_fd(io_handle) {
            Ok(fd) => fd,
            Err(_) => return UserRet::from_error(ErrNo::ENOMEM),
        };
        if cloexec {
            if vfs::fd::set_fd_flags(fd, FD_CLOEXEC).is_err() {
                return UserRet::from_error(ErrNo::EBADF);
            }
        }
        crate::unix_sock::register(fd, sock);
        return UserRet::from_success(fd);
    }

    if domain != AF_INET {
        return UserRet::from_error(ErrNo::EAFNOSUPPORT);
    }

    let cloexec = typ & SOCK_CLOEXEC != 0;
    let status_flags = if typ & SOCK_NONBLOCK != 0 {
        SOCK_NONBLOCK
    } else {
        0
    };
    typ &= !(SOCK_NONBLOCK | SOCK_CLOEXEC);

    let socket_result = match (typ, protocol) {
        (SOCK_STREAM, 0 | IPPROTO_TCP) => SocketRef::new_tcp(status_flags),
        (SOCK_DGRAM, 0 | IPPROTO_UDP) => SocketRef::new_udp(status_flags),
        (SOCK_STREAM | SOCK_DGRAM, _) => return UserRet::from_error(ErrNo::EPROTONOSUPPORT),
        (SOCK_RAW, _) => return UserRet::from_error(ErrNo::EPROTONOSUPPORT),
        _ => return UserRet::from_error(ErrNo::EINVAL),
    };

    let socket_ref = match socket_result {
        Ok(socket) => socket,
        Err(_) => return UserRet::from_error(ErrNo::ENOMEM),
    };

    let io_handle = match socket_ref.into_vfs_handle() {
        Ok(handle) => handle,
        Err(_) => return UserRet::from_error(ErrNo::ENOTSOCK),
    };

    let fd = match vfs::fd::alloc_fd(io_handle) {
        Ok(fd) => fd,
        Err(_) => return UserRet::from_error(ErrNo::ENOMEM),
    };
    if cloexec {
        if vfs::fd::set_fd_flags(fd, FD_CLOEXEC).is_err() {
            let _ = vfs::fd::close_fd(fd);
            return UserRet::from_error(ErrNo::EBADF);
        }
    }
    UserRet::from_success(fd)
}

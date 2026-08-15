//! `socket(2)`：创建 socket 并分配 fd。

//! 本模块代码由AI完成
use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use network::{SocketDomain, SocketRef};

const AF_INET : usize = 2;
#[cfg(feature = "ipv6")]
const AF_INET6 : usize = 10;
const AF_UNIX : usize = 1;
const SOCK_STREAM : usize = 1;
const SOCK_DGRAM : usize = 2;
const SOCK_RAW : usize = 3;
const SOCK_SEQPACKET : usize = 5;
const SOCK_NONBLOCK : usize = 0o4000;
const SOCK_CLOEXEC : usize = 0o2000000;
const FD_CLOEXEC : usize = 1;
const IPPROTO_TCP : usize = 6;
const IPPROTO_UDP : usize = 17;
const IPPROTO_ICMP : usize = 1;
#[cfg(feature = "ipv6")]
const IPPROTO_ICMPV6 : usize = 58;

// 本方法代码由AI完成
pub(crate) fn sys_socket(args : SyscallArgs) -> UserRet {
    let domain = args.arg(0);
    let mut typ = args.arg(1);
    let protocol = args.arg(2);

    if typ & !(0xF | SOCK_NONBLOCK | SOCK_CLOEXEC) != 0 {
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
        if !matches!(typ,
                     SOCK_STREAM | SOCK_DGRAM | SOCK_SEQPACKET) ||
           protocol != 0
        {
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

    let socket_domain = match domain {
        AF_INET => SocketDomain::Ipv4,
        #[cfg(feature = "ipv6")]
        AF_INET6 => SocketDomain::Ipv6,
        _ => return UserRet::from_error(ErrNo::EAFNOSUPPORT),
    };
    let cloexec = typ & SOCK_CLOEXEC != 0;
    let status_flags = if typ & SOCK_NONBLOCK != 0 {
        SOCK_NONBLOCK
    } else {
        0
    };
    typ &= !(SOCK_NONBLOCK | SOCK_CLOEXEC);

    let socket_result = match (typ, protocol) {
        (SOCK_STREAM, 0 | IPPROTO_TCP) => SocketRef::new_tcp(socket_domain, status_flags),
        (SOCK_DGRAM, 0 | IPPROTO_UDP) => SocketRef::new_udp(socket_domain, status_flags),
        (SOCK_RAW, IPPROTO_ICMP) if socket_domain == SocketDomain::Ipv4 => {
            SocketRef::new_icmp(socket_domain, status_flags)
        }
        #[cfg(feature = "ipv6")]
        (SOCK_RAW, IPPROTO_ICMPV6) if socket_domain == SocketDomain::Ipv6 => {
            SocketRef::new_icmp(socket_domain, status_flags)
        }
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

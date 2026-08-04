//! `getsockname(2)` / `getpeername(2)` — 获取 socket 地址。

//! 本模块代码由AI完成
use crate::socket_fd;
use crate::user_copy::copy_to_user_struct;
use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;

#[repr(C)]
#[derive(Copy, Clone)]
// 本结构代码由AI完成
struct SockAddrIn {
    sin_family : u16,
    sin_port : u16,
    sin_addr : [u8; 4],
    sin_zero : [u8; 8],
}

// 本方法代码由AI完成
pub(crate) fn sys_getsockname(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let addr_ptr = args.arg(1);
    let addrlen_ptr = args.arg(2);

    if crate::unix_sock::is_unix_fd(fd) {
        return match crate::unix_sock::getsockname(fd, addr_ptr, addrlen_ptr) {
            Ok(()) => UserRet::from_success(0),
            Err(e) => UserRet::from_error(e),
        };
    }

    let socket = match socket_fd::lookup_or_errno(fd) {
        Ok(s) => s,
        Err(e) => return UserRet::from_error(e),
    };

    let endpoint = match socket.local_endpoint() {
        Ok(endpoint) => endpoint,
        Err(_) => return UserRet::from_error(ErrNo::ENOTSOCK),
    };

    let addr = SockAddrIn { sin_family : 2, // AF_INET
                            sin_port : endpoint.port
                                               .to_be(),
                            sin_addr : endpoint.address,
                            sin_zero : [0; 8] };

    if addr_ptr != 0 && addrlen_ptr != 0 {
        if let Ok(addrlen_val) = crate::user_copy::copy_from_user_struct::<u32>(addrlen_ptr) {
            let write_len : usize = core::mem::size_of::<SockAddrIn>().min(addrlen_val as usize);
            let addr_bytes = unsafe {
                core::slice::from_raw_parts(&addr as *const SockAddrIn as *const u8,
                                            write_len)
            };
            let _ = crate::user_copy::copy_to_user(addr_ptr, addr_bytes);
            let _ = copy_to_user_struct(addrlen_ptr, &(write_len as u32));
        }
    }

    UserRet::from_success(0)
}

// 本方法代码由AI完成
pub(crate) fn sys_getpeername(args : SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let addr_ptr = args.arg(1);
    let addrlen_ptr = args.arg(2);

    if crate::unix_sock::is_unix_fd(fd) {
        return match crate::unix_sock::getpeername(fd, addr_ptr, addrlen_ptr) {
            Ok(()) => UserRet::from_success(0),
            Err(e) => UserRet::from_error(e),
        };
    }

    let socket = match socket_fd::lookup_or_errno(fd) {
        Ok(s) => s,
        Err(e) => return UserRet::from_error(e),
    };

    let endpoint = match socket.peer_endpoint() {
        Ok(endpoint) => endpoint,
        Err(_) => return UserRet::from_error(ErrNo::ENOTCONN),
    };

    let addr = SockAddrIn { sin_family : 2, // AF_INET
                            sin_port : endpoint.port
                                               .to_be(),
                            sin_addr : endpoint.address,
                            sin_zero : [0; 8] };

    if addr_ptr != 0 && addrlen_ptr != 0 {
        if let Ok(addrlen_val) = crate::user_copy::copy_from_user_struct::<u32>(addrlen_ptr) {
            let write_len : usize = core::mem::size_of::<SockAddrIn>().min(addrlen_val as usize);
            let addr_bytes = unsafe {
                core::slice::from_raw_parts(&addr as *const SockAddrIn as *const u8,
                                            write_len)
            };
            let _ = crate::user_copy::copy_to_user(addr_ptr, addr_bytes);
            let _ = copy_to_user_struct(addrlen_ptr, &(write_len as u32));
        }
    }

    UserRet::from_success(0)
}

//! `getsockname(2)` / `getpeername(2)` — 获取 socket 地址。

//! 本模块代码由AI完成
use crate::socket_fd;
use crate::user_copy::{copy_from_user_struct, copy_to_user, copy_to_user_struct};
use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;

#[repr(C)]
#[derive(Copy, Clone)]
// 本结构代码由AI完成
struct SockAddrIn {
    /// 地址族。
    sin_family : u16,
    /// 网络字节序端口。
    sin_port : u16,
    /// IPv4 地址。
    sin_addr : [u8; 4],
    /// ABI 填充。
    sin_zero : [u8; 8],
}

fn copy_socket_address(addr_ptr : usize,
                       addrlen_ptr : usize,
                       addr : &SockAddrIn)
                       -> Result<(), ErrNo> {
    if addr_ptr == 0 || addrlen_ptr == 0 {
        return Err(ErrNo::EFAULT);
    }
    let supplied = copy_from_user_struct::<u32>(addrlen_ptr)? as usize;
    // socklen_t 是无符号类型，但 Linux 拒绝由负 int 转换得到的巨大长度。
    if supplied > i32::MAX as usize {
        return Err(ErrNo::EINVAL);
    }
    let actual = core::mem::size_of::<SockAddrIn>();
    let write_len = actual.min(supplied);
    if write_len != 0 {
        let bytes = unsafe {
            core::slice::from_raw_parts(addr as *const SockAddrIn as *const u8, write_len)
        };
        copy_to_user(addr_ptr, bytes)?;
    }
    // getsockname/getpeername 的 addrlen 是 value-result 参数；即使用户缓冲区
    // 被截断，也必须回写完整地址所需的真实大小。
    copy_to_user_struct(addrlen_ptr, &(actual as u32))
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

    match copy_socket_address(addr_ptr, addrlen_ptr, &addr) {
        Ok(()) => UserRet::from_success(0),
        Err(error) => UserRet::from_error(error),
    }
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

    match copy_socket_address(addr_ptr, addrlen_ptr, &addr) {
        Ok(()) => UserRet::from_success(0),
        Err(error) => UserRet::from_error(error),
    }
}

//! `capget(2)` / `capset(2)` 最小实现：供 LTP 探测 POSIX capabilities。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::user_copy::{copy_from_user_struct, copy_to_user_struct};

const LINUX_CAPABILITY_VERSION_3: u32 = 0x2008_0522;
const CAP_ALL: u32 = 0xFFFF_FFFF;

#[repr(C)]
#[derive(Clone, Copy)]
struct CapUserHeader {
    version: u32,
    pid: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CapUserData {
    effective: u32,
    permitted: u32,
    inheritable: u32,
}

pub(crate) fn sys_capget(args: SyscallArgs) -> UserRet {
    let hdr_ptr = args.arg(0);
    let data_ptr = args.arg(1);
    if hdr_ptr == 0 || data_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let mut hdr: CapUserHeader = match copy_from_user_struct(hdr_ptr) {
        Ok(h) => h,
        Err(e) => return UserRet::from_error(e),
    };

    if hdr.version != 0 && hdr.version != LINUX_CAPABILITY_VERSION_3 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let pid = if hdr.pid == 0 {
        task::current_task_id().unwrap_or(0) as i32
    } else {
        hdr.pid
    };
    if hdr.pid != 0 && pid as usize != task::current_task_id().unwrap_or(0) {
        return UserRet::from_error(ErrNo::ESRCH);
    }

    hdr.version = LINUX_CAPABILITY_VERSION_3;
    hdr.pid = pid;
    if copy_to_user_struct(hdr_ptr, &hdr).is_err() {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let caps = CapUserData {
        effective: CAP_ALL,
        permitted: CAP_ALL,
        inheritable: 0,
    };
    if copy_to_user_struct(data_ptr, &caps).is_err() {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let bound = CapUserData {
        effective: CAP_ALL,
        permitted: CAP_ALL,
        inheritable: 0,
    };
    let bound_ptr = data_ptr + core::mem::size_of::<CapUserData>();
    if copy_to_user_struct(bound_ptr, &bound).is_err() {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    UserRet::from_success(0)
}

pub(crate) fn sys_capset(args: SyscallArgs) -> UserRet {
    let hdr_ptr = args.arg(0);
    let data_ptr = args.arg(1);
    if hdr_ptr == 0 || data_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let hdr: CapUserHeader = match copy_from_user_struct(hdr_ptr) {
        Ok(h) => h,
        Err(e) => return UserRet::from_error(e),
    };

    if hdr.version != LINUX_CAPABILITY_VERSION_3 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let pid = if hdr.pid == 0 {
        task::current_task_id().unwrap_or(0) as i32
    } else {
        hdr.pid
    };
    if pid as usize != task::current_task_id().unwrap_or(0) {
        return UserRet::from_error(ErrNo::EPERM);
    }

    let cred = cred::current_credentials();
    if cred.effective_uid.0 != 0 {
        return UserRet::from_error(ErrNo::EPERM);
    }

    let _caps: CapUserData = match copy_from_user_struct(data_ptr) {
        Ok(c) => c,
        Err(e) => return UserRet::from_error(e),
    };

    UserRet::from_success(0)
}

//! `capget(2)` / `capset(2)` 最小实现：供 LTP 探测 POSIX capabilities。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use task::ProcessId;

use crate::user_copy::{copy_from_user_struct, copy_to_user_struct};

const LINUX_CAPABILITY_VERSION_1: u32 = 0x1998_0330;
const LINUX_CAPABILITY_VERSION_2: u32 = 0x2007_0522;
const LINUX_CAPABILITY_VERSION_3: u32 = 0x2008_0522;
const CAP_NET_RAW: u32 = 1 << 13;

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

fn cap_target_exists(pid: i32) -> bool {
    if pid == 0 {
        return task::current_task_id().is_some();
    }
    let raw = pid as usize;
    task::process_task_snapshot(raw).is_some()
        || task::process_snapshot(ProcessId::from_raw(raw)).is_some()
}

fn root_caps() -> CapUserData {
    let mask = 0xFFFF_FFFF & !CAP_NET_RAW;
    CapUserData {
        effective: mask,
        permitted: mask,
        inheritable: 0,
    }
}

fn cap_version_ok(version: u32) -> bool {
    version == LINUX_CAPABILITY_VERSION_1
        || version == LINUX_CAPABILITY_VERSION_2
        || version == LINUX_CAPABILITY_VERSION_3
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

    if hdr.version == 0 {
        hdr.version = LINUX_CAPABILITY_VERSION_3;
    }
    if !cap_version_ok(hdr.version) {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    if !cap_target_exists(hdr.pid) {
        return UserRet::from_error(ErrNo::ESRCH);
    }

    let reported_pid = if hdr.pid == 0 {
        task::current_task_id().unwrap_or(0) as i32
    } else {
        hdr.pid
    };

    hdr.pid = reported_pid;
    if copy_to_user_struct(hdr_ptr, &hdr).is_err() {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let caps = root_caps();
    if copy_to_user_struct(data_ptr, &caps).is_err() {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    if hdr.version == LINUX_CAPABILITY_VERSION_3 {
        let bound_ptr = data_ptr + core::mem::size_of::<CapUserData>();
        if copy_to_user_struct(bound_ptr, &caps).is_err() {
            return UserRet::from_error(ErrNo::EFAULT);
        }
    }

    UserRet::from_success(0)
}

pub(crate) fn sys_capset(args: SyscallArgs) -> UserRet {
    let hdr_ptr = args.arg(0);
    let data_ptr = args.arg(1);
    if hdr_ptr == 0 || data_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let mut hdr: CapUserHeader = match copy_from_user_struct(hdr_ptr) {
        Ok(h) => h,
        Err(e) => return UserRet::from_error(e),
    };

    if hdr.version == 0 {
        hdr.version = LINUX_CAPABILITY_VERSION_3;
    }
    if !cap_version_ok(hdr.version) {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    if !cap_target_exists(hdr.pid) {
        return UserRet::from_error(ErrNo::ESRCH);
    }

    let current = match task::current_task_id() {
        Some(id) => id,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    if hdr.pid != 0 && hdr.pid as usize != current {
        let current_pid = task::current_process_task_snapshot()
            .map(|snapshot| snapshot.pid.raw())
            .unwrap_or(usize::MAX);
        if hdr.pid as usize != current_pid {
            return UserRet::from_error(ErrNo::EPERM);
        }
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

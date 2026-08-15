//! `capget(2)` / `capset(2)` 最小实现：供 LTP 探测 POSIX capabilities。

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use task::ProcessCaps;
use task::ProcessId;

use crate::user_copy::{copy_from_user_struct, copy_to_user_struct};

const LINUX_CAPABILITY_VERSION_1 : u32 = 0x1998_0330;
const LINUX_CAPABILITY_VERSION_2 : u32 = 0x2007_1026;
const LINUX_CAPABILITY_VERSION_3 : u32 = 0x2008_0522;

#[repr(C)]
#[derive(Clone, Copy)]
struct CapUserHeader {
    version : u32,
    pid : i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CapUserData {
    effective : u32,
    permitted : u32,
    inheritable : u32,
}

fn cap_target_exists(pid : i32) -> bool {
    if pid == 0 {
        return task::current_task_id().is_some();
    }
    let raw = pid as usize;
    task::process_task_snapshot(raw).is_some() ||
    task::process_snapshot(ProcessId::from_raw(raw)).is_some()
}

fn cap_version_ok(version : u32) -> bool {
    version == LINUX_CAPABILITY_VERSION_1 ||
    version == LINUX_CAPABILITY_VERSION_2 ||
    version == LINUX_CAPABILITY_VERSION_3
}

fn write_preferred_version(hdr_ptr : usize, mut hdr : CapUserHeader) -> UserRet {
    hdr.version = LINUX_CAPABILITY_VERSION_3;
    match copy_to_user_struct(hdr_ptr, &hdr) {
        Ok(()) => UserRet::from_error(ErrNo::EINVAL),
        Err(e) => UserRet::from_error(e),
    }
}

fn cap_data_words(version : u32) -> usize {
    if version == LINUX_CAPABILITY_VERSION_1 {
        1
    } else {
        2
    }
}

/// 读取目标进程的 capability 三集合；`pid == 0` 表示当前进程。
fn process_caps_of(pid : i32) -> CapUserData {
    let target = if pid == 0 {
        task::current_process_task_snapshot().map(|snapshot| snapshot.pid)
    } else {
        Some(ProcessId::from_raw(pid as usize))
    };
    match target.and_then(|process_pid| task::process_caps(process_pid)) {
        Some(caps) => CapUserData { effective : caps.effective,
                                    permitted : caps.permitted,
                                    inheritable : caps.inheritable },
        None => CapUserData { effective : 0,
                              permitted : 0,
                              inheritable : 0 },
    }
}

pub(crate) fn cap_bset_read(cap : usize) -> UserRet {
    if cap >= 64 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    UserRet::from_success(1)
}

pub(crate) fn cap_bset_drop(cap : usize) -> UserRet {
    if cap >= 64 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    UserRet::from_error(ErrNo::EPERM)
}

pub(crate) fn sys_capget(args : SyscallArgs) -> UserRet {
    let hdr_ptr = args.arg(0);
    let data_ptr = args.arg(1);
    // 只要求 header 指针非空；`data == NULL` 是合法的版本探测调用
    // （libcap-ng 用 `capget(&hdr, NULL)` 探测版本），不能因此返回 EFAULT。
    if hdr_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let mut hdr : CapUserHeader = match copy_from_user_struct(hdr_ptr) {
        Ok(h) => h,
        Err(e) => return UserRet::from_error(e),
    };

    if !cap_version_ok(hdr.version) {
        return write_preferred_version(hdr_ptr, hdr);
    }

    if hdr.pid < 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    if !cap_target_exists(hdr.pid) {
        return UserRet::from_error(ErrNo::ESRCH);
    }

    // Linux 允许 `capget(&hdr, NULL)` 作为版本探测：只确认版本受支持，
    // 不写数据即返回成功（libcap-ng 的 capng_apply 依赖此语义）。
    if data_ptr == 0 {
        return UserRet::from_success(0);
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

    let caps = process_caps_of(hdr.pid);
    if copy_to_user_struct(data_ptr, &caps).is_err() {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    for word in 1..cap_data_words(hdr.version) {
        let zero = CapUserData { effective : 0,
                                 permitted : 0,
                                 inheritable : 0 };
        let ptr = data_ptr + word * core::mem::size_of::<CapUserData>();
        if copy_to_user_struct(ptr, &zero).is_err() {
            return UserRet::from_error(ErrNo::EFAULT);
        }
    }

    UserRet::from_success(0)
}

pub(crate) fn sys_capset(args : SyscallArgs) -> UserRet {
    let hdr_ptr = args.arg(0);
    let data_ptr = args.arg(1);
    if hdr_ptr == 0 || data_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let hdr : CapUserHeader = match copy_from_user_struct(hdr_ptr) {
        Ok(h) => h,
        Err(e) => return UserRet::from_error(e),
    };

    if !cap_version_ok(hdr.version) {
        return write_preferred_version(hdr_ptr, hdr);
    }

    if hdr.pid < 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    if !cap_target_exists(hdr.pid) {
        return UserRet::from_error(ErrNo::ESRCH);
    }

    let current_pid =
        task::current_process_task_snapshot().map(|snapshot| snapshot.pid)
                                             .unwrap_or(ProcessId::from_raw(usize::MAX));
    // 仅允许进程设置自己的 capability（pid == 0 或等于当前进程）。
    if hdr.pid != 0 && ProcessId::from_raw(hdr.pid as usize) != current_pid {
        return UserRet::from_error(ErrNo::EPERM);
    }

    let caps : CapUserData = match copy_from_user_struct(data_ptr) {
        Ok(c) => c,
        Err(e) => return UserRet::from_error(e),
    };
    // 自洽性：effective / inheritable 必须是 requested permitted 的子集。
    if caps.effective & !caps.permitted != 0 {
        return UserRet::from_error(ErrNo::EPERM);
    }
    if caps.inheritable & !caps.permitted != 0 {
        return UserRet::from_error(ErrNo::EPERM);
    }

    let cred = cred::current_credentials();
    let is_root = cred.effective_uid.0 == 0;
    let current = task::process_caps(current_pid).unwrap_or(ProcessCaps::ROOT);
    // 非 root 只能把 requested 集合限制在当前 permitted 的子集内（不扩大）。
    // 配合 PR_SET_KEEPCAPS，setuid 之后仍可重设 permitted 子集（setpriv 的
    // “reactivate capabilities” 流程）；euid == 0 的 root 可任意设置。
    if !is_root && caps.permitted & !current.permitted != 0 {
        return UserRet::from_error(ErrNo::EPERM);
    }

    let stored = ProcessCaps { effective : caps.effective,
                               permitted : caps.permitted,
                               inheritable : caps.inheritable };
    if task::set_process_caps(current_pid, stored).is_err() {
        return UserRet::from_error(ErrNo::EPERM);
    }

    UserRet::from_success(0)
}

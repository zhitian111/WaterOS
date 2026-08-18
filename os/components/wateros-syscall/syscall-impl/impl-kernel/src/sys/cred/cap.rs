//! `capget(2)` / `capset(2)` 最小实现：供 LTP 探测 POSIX capabilities。

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use task::ProcessCaps;
use task::ProcessId;
use task::ThreadId;

use crate::user_copy::{copy_from_user_struct, copy_to_user_struct};

const LINUX_CAPABILITY_VERSION_1 : u32 = 0x1998_0330;
const LINUX_CAPABILITY_VERSION_2 : u32 = 0x2007_1026;
const LINUX_CAPABILITY_VERSION_3 : u32 = 0x2008_0522;

#[repr(C)]
#[derive(Clone, Copy)]
struct CapUserHeader {
    /// libcap 使用的 capability ABI 版本号。
    version : u32,
    /// 目标进程 ID；0 表示当前进程。
    pid : i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CapUserData {
    /// 当前可生效的 capability 位图（低 32 位）。
    effective : u32,
    /// 允许进程提升到 effective 的 capability 位图。
    permitted : u32,
    /// 可由子进程继承的 capability 位图。
    inheritable : u32,
}

fn cap_target_process(pid : i32) -> Option<ProcessId> {
    if pid == 0 {
        return task::current_process_task_snapshot().map(|snapshot| snapshot.pid);
    }
    let raw = pid as usize;
    let process_pid = ProcessId::from_raw(raw);
    if task::process_snapshot(process_pid).is_some() {
        return Some(process_pid);
    }
    let task_id = task::task_id_for_thread(ThreadId::from_raw(raw))?;
    task::process_task_snapshot(task_id).map(|snapshot| snapshot.pid)
}

fn cap_target_exists(pid : i32) -> bool {
    cap_target_process(pid).is_some()
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
    let target = cap_target_process(pid);
    match target.and_then(|process_pid| task::process_caps(process_pid)) {
        Some(caps) => CapUserData { effective : caps.effective,
                                    permitted : caps.permitted,
                                    inheritable : caps.inheritable },
        None => CapUserData { effective : 0,
                              permitted : 0,
                              inheritable : 0 },
    }
}

/// 当前进程的 capability 状态（含 bounding）；缺失时按 root 兜底。
fn current_process_caps() -> ProcessCaps {
    task::current_process_task_snapshot().map(|snapshot| snapshot.pid)
                                         .and_then(|pid| task::process_caps(pid))
                                         .unwrap_or(ProcessCaps::ROOT)
}

pub(crate) fn cap_bset_read(cap : usize) -> UserRet {
    // WaterOS 只支持低 32 位 capability；超出范围按 Linux 语义返回 EINVAL
    // （libcap 的 cap_last_cap() 二分探测依赖这一点）。
    if cap >= 32 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let in_set = ((current_process_caps().bounding >> cap) & 1) as usize;
    UserRet::from_success(in_set)
}

pub(crate) fn cap_bset_drop(cap : usize) -> UserRet {
    if cap >= 32 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let Some(current_pid) = task::current_process_task_snapshot().map(|snapshot| snapshot.pid)
    else {
        return UserRet::from_error(ErrNo::ESRCH);
    };
    let caps = task::process_caps(current_pid).unwrap_or(ProcessCaps::ROOT);
    let cred = cred::current_credentials();
    // Linux cap_bset_drop：需要 euid==0 或 effective 持有 CAP_SETPCAP。
    if cred.effective_uid.0 != 0 && caps.effective & ProcessCaps::CAP_SETPCAP == 0 {
        return UserRet::from_error(ErrNo::EPERM);
    }
    // 从 bounding 去掉后，同步把各集合中该 cap 剪掉（Linux 会 prune）。
    let bit = 1u32 << cap;
    let stored = ProcessCaps { effective : caps.effective & !bit,
                               permitted : caps.permitted & !bit,
                               inheritable : caps.inheritable & !bit,
                               bounding : caps.bounding & !bit };
    if task::set_process_caps(current_pid, stored).is_err() {
        return UserRet::from_error(ErrNo::EPERM);
    }
    UserRet::from_success(0)
}

pub(crate) fn sys_capget(args : SyscallArgs) -> UserRet {
    let hdr_ptr = args.arg(0);
    let data_ptr = args.arg(1);
    // 只要求 header 指针非空；`data == NULL` 是合法的版本探测调用
    // （libcap-ng 用 `capget(&hdr, NULL)` 探测版本），不能因此返回 EFAULT。
    if hdr_ptr == 0 {
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

    // Linux 允许 `capget(&hdr, NULL)` 作为版本探测：只确认版本受支持，
    // 不写数据即返回成功（libcap-ng 的 capng_apply 依赖此语义）。
    if data_ptr == 0 {
        return UserRet::from_success(0);
    }

    // 合法版本的 capget 不重写 header。尤其 pid=0 必须保持为 0；内部 TaskId
    // 不是 Linux PID，把它写回会让随后复用 header 的 capset 错设其它目标。
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
    // Linux 允许用同一线程组中的 TID 指定当前进程；统一映射后再判断归属。
    if cap_target_process(hdr.pid) != Some(current_pid) {
        return UserRet::from_error(ErrNo::EPERM);
    }

    let caps : CapUserData = match copy_from_user_struct(data_ptr) {
        Ok(c) => c,
        Err(e) => return UserRet::from_error(e),
    };
    // 自洽性：effective 必须是 requested permitted 的子集（Linux capset 强制）。
    if caps.effective & !caps.permitted != 0 {
        return UserRet::from_error(ErrNo::EPERM);
    }
    // WaterOS 只支持低 32 位 capability（word 0）；V2/V3 的第二个
    // CapUserData（cap 32..63）请求非零时明确拒绝，避免静默忽略。
    if cap_data_words(hdr.version) > 1 {
        let word1_ptr = data_ptr + core::mem::size_of::<CapUserData>();
        let word1 : CapUserData = match copy_from_user_struct(word1_ptr) {
            Ok(c) => c,
            Err(e) => return UserRet::from_error(e),
        };
        if word1.effective != 0 || word1.permitted != 0 || word1.inheritable != 0 {
            return UserRet::from_error(ErrNo::EPERM);
        }
    }

    let current = task::process_caps(current_pid).unwrap_or(ProcessCaps::ROOT);
    // 所有集合都不能超出 bounding set（Linux 对 root 同样生效）。
    if caps.permitted & !current.bounding != 0 || caps.inheritable & !current.bounding != 0 {
        return UserRet::from_error(ErrNo::EPERM);
    }
    // Linux cap_capset：特权与否看 effective 是否持有 CAP_SETPCAP，不是
    // euid==0。root 默认有 CAP_SETPCAP 可任意设置；但可先 capset 去掉
    // SETPCAP 再进入受限场景（LTP capset03 就是这样测的）。
    let privileged = current.effective & ProcessCaps::CAP_SETPCAP != 0;
    if !privileged {
        // permitted 只减不增（Linux）；配合 PR_SET_KEEPCAPS，setuid 之后仍可
        // 重设 permitted 子集（setpriv 的 “reactivate capabilities” 流程）。
        if caps.permitted & !current.permitted != 0 {
            return UserRet::from_error(ErrNo::EPERM);
        }
        // inheritable 只减不增（Linux capset 语义：非特权进程不能新增
        // inheritable cap，避免通过 exec 传递）。
        if caps.inheritable & !current.inheritable != 0 {
            return UserRet::from_error(ErrNo::EPERM);
        }
    }

    let stored = ProcessCaps { effective : caps.effective,
                               permitted : caps.permitted,
                               inheritable : caps.inheritable,
                               bounding : current.bounding };
    if task::set_process_caps(current_pid, stored).is_err() {
        return UserRet::from_error(ErrNo::EPERM);
    }

    UserRet::from_success(0)
}

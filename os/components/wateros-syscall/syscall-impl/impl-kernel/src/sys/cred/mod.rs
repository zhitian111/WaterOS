//! 用户/组凭证与 capability 系统调用：uid/gid 系列 + capget/capset。

pub(crate) mod cap;
mod groups;
mod setid;

pub(crate) use cap::{sys_capget, sys_capset};

// ── 原 cred.rs 内容 ────────────────────────────────────────
use crate::user_copy::{copy_from_user, copy_from_user_struct, copy_to_user, copy_to_user_struct};
use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use cred::api::{Gid, Uid, SUPPLEMENTARY_GROUP_COUNT};

use groups::{plan_getgroups, valid_setgroups_size, GetGroupsPlan};
use setid::{plan_set_id, plan_set_re_id, plan_set_res_id, IdTriplet};
use task::ProcessCaps;

/// 当前进程 effective capability 掩码。
///
/// 用于替代部分“仅 euid==0”的特权判定：非 root 但 effective 集合持有
/// CAP_SETUID / CAP_SETGID 的进程（如 setpriv 在 PR_SET_KEEPCAPS +
/// setresuid 之后）仍可切换 uid/gid（Linux 语义）。
fn current_effective_caps() -> u32 {
    task::current_process_task_snapshot().and_then(|snapshot| task::process_caps(snapshot.pid))
                                         .map(|caps| caps.effective)
                                         .unwrap_or(0)
}

/// uid 系列 set*id 特权：euid==0 或 effective 含 CAP_SETUID。
fn uid_privileged() -> bool {
    let cred = cred::current_credentials();
    cred.effective_uid.0 == 0 || (current_effective_caps() & ProcessCaps::CAP_SETUID) != 0
}

/// gid 系列 set*id / setgroups 特权：euid==0 或 effective 含 CAP_SETGID。
fn gid_privileged() -> bool {
    let cred = cred::current_credentials();
    cred.effective_uid.0 == 0 || (current_effective_caps() & ProcessCaps::CAP_SETGID) != 0
}

fn current_uid_triplet() -> (IdTriplet, bool) {
    let current = cred::current_credentials();
    (IdTriplet { real : current.real_uid.0,
                 effective : current.effective_uid
                                    .0,
                 saved : current.saved_uid.0 },
     uid_privileged())
}

fn current_gid_triplet() -> (IdTriplet, bool) {
    let current = cred::current_credentials();
    (IdTriplet { real : current.real_gid.0,
                 effective : current.effective_gid
                                    .0,
                 saved : current.saved_gid.0 },
     gid_privileged())
}

fn apply_uid_triplet(ids : IdTriplet) {
    let old_euid = cred::current_credentials().effective_uid
                                                 .0;
    cred::set_resuid(Some(Uid(ids.real)),
                     Some(Uid(ids.effective)),
                     Some(Uid(ids.saved)));
    let new_euid = ids.effective;
    if old_euid == new_euid {
        return;
    }
    // Linux setxuid 的 capability 转换规则：
    // - euid 0 -> 非0：清 effective；未设 KEEPCAPS 时连 permitted 一起清。
    // - euid 非0 -> 0：effective 恢复为 permitted。
    let Some(pid) = task::current_process_task_snapshot().map(|snapshot| snapshot.pid) else {
        return;
    };
    let Some(mut caps) = task::process_caps(pid) else {
        return;
    };
    if old_euid == 0 {
        caps.effective = 0;
        if !task::process_keep_caps(pid).unwrap_or(false) {
            caps.permitted = 0;
        }
    } else if new_euid == 0 {
        caps.effective = caps.permitted;
    }
    let _ = task::set_process_caps(pid, caps);
}

fn apply_gid_triplet(ids : IdTriplet) {
    cred::set_resgid(Some(Gid(ids.real)),
                     Some(Gid(ids.effective)),
                     Some(Gid(ids.saved)));
}

fn write_id_triplet(pointers : [usize; 3], values : [u32; 3]) -> Result<(), ErrNo> {
    for (pointer, value) in pointers.into_iter()
                                    .zip(values)
    {
        copy_to_user_struct(pointer, &value)?;
    }
    Ok(())
}

pub(crate) fn sys_getuid() -> UserRet {
    let cred = cred::current_credentials();
    UserRet::from_success(cred.real_uid.0 as usize)
}
pub(crate) fn sys_geteuid() -> UserRet {
    let cred = cred::current_credentials();
    UserRet::from_success(cred.effective_uid.0 as usize)
}
pub(crate) fn sys_getgid() -> UserRet {
    let cred = cred::current_credentials();
    UserRet::from_success(cred.real_gid.0 as usize)
}
pub(crate) fn sys_getegid() -> UserRet {
    let cred = cred::current_credentials();
    UserRet::from_success(cred.effective_gid.0 as usize)
}
pub(crate) fn sys_getgroups(args : SyscallArgs) -> UserRet {
    let size = args.arg(0);
    let list_ptr = args.arg(1);
    let cred = cred::current_credentials();
    let n = match plan_getgroups(size, cred.supplementary_group_len) {
        Some(GetGroupsPlan::Query(count)) => return UserRet::from_success(count),
        Some(GetGroupsPlan::Copy(count)) => count,
        None => return UserRet::from_error(ErrNo::EINVAL),
    };
    if n > 0 {
        if list_ptr == 0 {
            return UserRet::from_error(ErrNo::EFAULT);
        }
        let raw : alloc::vec::Vec<u32> = cred.supplementary_groups[..n].iter()
                                                                       .map(|g| g.0)
                                                                       .collect();
        let bytes = unsafe {
            core::slice::from_raw_parts(raw.as_ptr() as *const u8,
                                        n * core::mem::size_of::<u32>())
        };
        if let Err(e) = copy_to_user(list_ptr, bytes) {
            return UserRet::from_error(e);
        }
    }
    UserRet::from_success(n)
}
pub(crate) fn sys_setgroups(args : SyscallArgs) -> UserRet {
    let size = args.arg(0);
    let list_ptr = args.arg(1);
    if !valid_setgroups_size(size, SUPPLEMENTARY_GROUP_COUNT) {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    // root 或 effective 集合持有 CAP_SETGID（setpriv --clear-groups 在
    // setresgid 之后以非 root euid 调用）。
    if !gid_privileged() {
        return UserRet::from_error(ErrNo::EPERM);
    }
    let count = size;
    if count > 0 && list_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let mut raw = alloc::vec![0u32; count];
    if count > 0 {
        let raw_bytes = unsafe {
            core::slice::from_raw_parts_mut(raw.as_mut_ptr() as *mut u8,
                                            count * core::mem::size_of::<u32>())
        };
        if let Err(e) = copy_from_user(raw_bytes, list_ptr) {
            return UserRet::from_error(e);
        }
    }
    let groups : alloc::vec::Vec<Gid> = raw.iter()
                                           .map(|v| Gid(*v))
                                           .collect();
    cred::set_supplementary_groups(groups.as_slice());
    UserRet::from_success(0)
}
pub(crate) fn sys_setuid(args : SyscallArgs) -> UserRet {
    let (current, privileged) = current_uid_triplet();
    let Some(next) = plan_set_id(current, args.arg(0) as u32, privileged) else {
        return UserRet::from_error(ErrNo::EPERM);
    };
    apply_uid_triplet(next);
    UserRet::from_success(0)
}
pub(crate) fn sys_setgid(args : SyscallArgs) -> UserRet {
    let (current, privileged) = current_gid_triplet();
    let Some(next) = plan_set_id(current, args.arg(0) as u32, privileged) else {
        return UserRet::from_error(ErrNo::EPERM);
    };
    apply_gid_triplet(next);
    UserRet::from_success(0)
}
pub(crate) fn sys_setreuid(args : SyscallArgs) -> UserRet {
    let ruid = if args.arg(0) == !0usize {
        None
    } else {
        Some(Uid(args.arg(0) as u32))
    };
    let euid = if args.arg(1) == !0usize {
        None
    } else {
        Some(Uid(args.arg(1) as u32))
    };
    let (current, privileged) = current_uid_triplet();
    let Some(next) = plan_set_re_id(current,
                                    ruid.map(|id| id.0),
                                    euid.map(|id| id.0),
                                    privileged)
    else {
        return UserRet::from_error(ErrNo::EPERM);
    };
    apply_uid_triplet(next);
    UserRet::from_success(0)
}
pub(crate) fn sys_setregid(args : SyscallArgs) -> UserRet {
    let rgid = if args.arg(0) == !0usize {
        None
    } else {
        Some(Gid(args.arg(0) as u32))
    };
    let egid = if args.arg(1) == !0usize {
        None
    } else {
        Some(Gid(args.arg(1) as u32))
    };
    let (current, privileged) = current_gid_triplet();
    let Some(next) = plan_set_re_id(current,
                                    rgid.map(|id| id.0),
                                    egid.map(|id| id.0),
                                    privileged)
    else {
        return UserRet::from_error(ErrNo::EPERM);
    };
    apply_gid_triplet(next);
    UserRet::from_success(0)
}
pub(crate) fn sys_setresuid(args : SyscallArgs) -> UserRet {
    let ruid = if args.arg(0) == !0usize {
        None
    } else {
        Some(Uid(args.arg(0) as u32))
    };
    let euid = if args.arg(1) == !0usize {
        None
    } else {
        Some(Uid(args.arg(1) as u32))
    };
    let suid = if args.arg(2) == !0usize {
        None
    } else {
        Some(Uid(args.arg(2) as u32))
    };
    let (current, privileged) = current_uid_triplet();
    let Some(next) = plan_set_res_id(current,
                                     ruid.map(|id| id.0),
                                     euid.map(|id| id.0),
                                     suid.map(|id| id.0),
                                     privileged)
    else {
        return UserRet::from_error(ErrNo::EPERM);
    };
    apply_uid_triplet(next);
    UserRet::from_success(0)
}
pub(crate) fn sys_setresgid(args : SyscallArgs) -> UserRet {
    let rgid = if args.arg(0) == !0usize {
        None
    } else {
        Some(Gid(args.arg(0) as u32))
    };
    let egid = if args.arg(1) == !0usize {
        None
    } else {
        Some(Gid(args.arg(1) as u32))
    };
    let sgid = if args.arg(2) == !0usize {
        None
    } else {
        Some(Gid(args.arg(2) as u32))
    };
    let (current, privileged) = current_gid_triplet();
    let Some(next) = plan_set_res_id(current,
                                     rgid.map(|id| id.0),
                                     egid.map(|id| id.0),
                                     sgid.map(|id| id.0),
                                     privileged)
    else {
        return UserRet::from_error(ErrNo::EPERM);
    };
    apply_gid_triplet(next);
    UserRet::from_success(0)
}
pub(crate) fn sys_getresuid(args : SyscallArgs) -> UserRet {
    let ruid_ptr = args.arg(0);
    let euid_ptr = args.arg(1);
    let suid_ptr = args.arg(2);
    let cred = cred::current_credentials();
    match write_id_triplet([ruid_ptr, euid_ptr, suid_ptr],
                           [cred.real_uid.0,
                            cred.effective_uid.0,
                            cred.saved_uid.0])
    {
        Ok(()) => UserRet::from_success(0),
        Err(error) => UserRet::from_error(error),
    }
}
pub(crate) fn sys_getresgid(args : SyscallArgs) -> UserRet {
    let rgid_ptr = args.arg(0);
    let egid_ptr = args.arg(1);
    let sgid_ptr = args.arg(2);
    let cred = cred::current_credentials();
    match write_id_triplet([rgid_ptr, egid_ptr, sgid_ptr],
                           [cred.real_gid.0,
                            cred.effective_gid.0,
                            cred.saved_gid.0])
    {
        Ok(()) => UserRet::from_success(0),
        Err(error) => UserRet::from_error(error),
    }
}

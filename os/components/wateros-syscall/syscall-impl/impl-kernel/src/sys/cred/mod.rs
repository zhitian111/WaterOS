//! 用户/组凭证与 capability 系统调用：uid/gid 系列 + capget/capset。

pub(crate) mod cap;

pub(crate) use cap::{sys_capget, sys_capset};

// ── 原 cred.rs 内容 ────────────────────────────────────────
use crate::user_copy::{copy_from_user, copy_from_user_struct, copy_to_user, copy_to_user_struct};
use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use cred::api::{Gid, Uid, SUPPLEMENTARY_GROUP_COUNT};

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
    if size == 0 {
        return UserRet::from_success(SUPPLEMENTARY_GROUP_COUNT as usize);
    }
    let n = cred.supplementary_group_len
                .min(size as usize);
    if n > 0 {
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
    if list_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let cred = cred::current_credentials();
    if cred.effective_uid.0 != 0 {
        return UserRet::from_error(ErrNo::EPERM);
    }
    let count = size.min(SUPPLEMENTARY_GROUP_COUNT as usize);
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
    let uid = Uid(args.arg(0) as u32);
    cred::set_uid(uid);
    UserRet::from_success(0)
}
pub(crate) fn sys_setgid(args : SyscallArgs) -> UserRet {
    let gid = Gid(args.arg(0) as u32);
    cred::set_gid(gid);
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
    cred::set_reuid(ruid, euid);
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
    cred::set_regid(rgid, egid);
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
    cred::set_resuid(ruid, euid, suid);
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
    cred::set_resgid(rgid, egid, sgid);
    UserRet::from_success(0)
}
pub(crate) fn sys_getresuid(args : SyscallArgs) -> UserRet {
    let ruid_ptr = args.arg(0);
    let euid_ptr = args.arg(1);
    let suid_ptr = args.arg(2);
    let cred = cred::current_credentials();
    let _ = copy_to_user_struct(ruid_ptr, &cred.real_uid.0);
    let _ = copy_to_user_struct(euid_ptr, &cred.effective_uid.0);
    let _ = copy_to_user_struct(suid_ptr, &cred.saved_uid.0);
    UserRet::from_success(0)
}
pub(crate) fn sys_getresgid(args : SyscallArgs) -> UserRet {
    let rgid_ptr = args.arg(0);
    let egid_ptr = args.arg(1);
    let sgid_ptr = args.arg(2);
    let cred = cred::current_credentials();
    let _ = copy_to_user_struct(rgid_ptr, &cred.real_gid.0);
    let _ = copy_to_user_struct(egid_ptr, &cred.effective_gid.0);
    let _ = copy_to_user_struct(sgid_ptr, &cred.saved_gid.0);
    UserRet::from_success(0)
}

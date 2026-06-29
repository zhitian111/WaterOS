//! 进程凭证相关系统调用：`getuid`/`geteuid`/`getgid`/`getegid`/`getgroups` 与 set*id。
//! 本模块代码由AI完成

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use cred::api::{Gid, Uid, SUPPLEMENTARY_GROUP_COUNT};

use crate::user_copy::{copy_from_user_struct, copy_to_user, copy_to_user_struct};

// 本方法代码由AI完成
pub(crate) fn sys_getuid() -> UserRet {
    let cred = cred::current_credentials();
    UserRet::from_success(cred.real_uid.0 as usize)
}

// 本方法代码由AI完成
pub(crate) fn sys_geteuid() -> UserRet {
    let cred = cred::current_credentials();
    UserRet::from_success(cred.effective_uid.0 as usize)
}

// 本方法代码由AI完成
pub(crate) fn sys_getgid() -> UserRet {
    let cred = cred::current_credentials();
    UserRet::from_success(cred.real_gid.0 as usize)
}

// 本方法代码由AI完成
pub(crate) fn sys_getegid() -> UserRet {
    let cred = cred::current_credentials();
    UserRet::from_success(cred.effective_gid.0 as usize)
}

// 本方法代码由AI完成
pub(crate) fn sys_getgroups(args: SyscallArgs) -> UserRet {
    let size = args.arg(0) as isize;
    let list_ptr = args.arg(1);

    if size < 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let cred = cred::current_credentials();
    let ngroups = cred.supplementary_group_count() as usize;

    if size == 0 {
        return UserRet::from_success(ngroups);
    }

    if list_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    if (size as usize) < ngroups {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let mut gid_buf = [0u32; SUPPLEMENTARY_GROUP_COUNT];
    for (i, g) in cred
        .supplementary_groups
        .iter()
        .take(ngroups)
        .enumerate()
    {
        gid_buf[i] = g.0;
    }
    let bytes = unsafe {
        core::slice::from_raw_parts(
            gid_buf.as_ptr() as *const u8,
            gid_buf.len() * core::mem::size_of::<u32>(),
        )
    };
    let write_len = (size as usize).min(gid_buf.len());
    if copy_to_user(
        list_ptr,
        &bytes[..write_len * core::mem::size_of::<u32>()],
    )
    .is_err()
    {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    UserRet::from_success(ngroups)
}

// 本方法代码由AI完成
pub(crate) fn sys_setgroups(args: SyscallArgs) -> UserRet {
    let size = args.arg(0) as isize;
    let list_ptr = args.arg(1);

    if size < 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let ngroups = size as usize;
    if ngroups > SUPPLEMENTARY_GROUP_COUNT {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if ngroups > 0 && list_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let mut groups = [Gid(0); SUPPLEMENTARY_GROUP_COUNT];
    for (i, group) in groups.iter_mut().take(ngroups).enumerate() {
        let gid = match copy_from_user_struct::<u32>(list_ptr + i * core::mem::size_of::<u32>()) {
            Ok(gid) => gid,
            Err(_) => return UserRet::from_error(ErrNo::EFAULT),
        };
        *group = Gid(gid);
    }
    cred::set_supplementary_groups(&groups[..ngroups]);
    UserRet::from_success(0)
}

// 本方法代码由AI完成
pub(crate) fn sys_setuid(args: SyscallArgs) -> UserRet {
    let uid = match parse_required_u32_id(args.arg(0)) {
        Some(uid) => Uid(uid),
        None => return UserRet::from_error(ErrNo::EINVAL),
    };
    let current = cred::current_credentials();
    if current.effective_uid.0 != 0
        && uid != current.real_uid
        && uid != current.effective_uid
        && uid != current.saved_uid
    {
        return UserRet::from_error(ErrNo::EPERM);
    }
    cred::set_uid(uid);
    UserRet::from_success(0)
}

// 本方法代码由AI完成
pub(crate) fn sys_setgid(args: SyscallArgs) -> UserRet {
    let gid = match parse_required_u32_id(args.arg(0)) {
        Some(gid) => Gid(gid),
        None => return UserRet::from_error(ErrNo::EINVAL),
    };
    let current = cred::current_credentials();
    if current.effective_uid.0 != 0
        && gid != current.real_gid
        && gid != current.effective_gid
        && gid != current.saved_gid
    {
        return UserRet::from_error(ErrNo::EPERM);
    }
    cred::set_gid(gid);
    UserRet::from_success(0)
}

// 本方法代码由AI完成
pub(crate) fn sys_setreuid(args: SyscallArgs) -> UserRet {
    let real_uid = match parse_optional_u32_id(args.arg(0)) {
        Some(uid) => uid.map(Uid),
        None => return UserRet::from_error(ErrNo::EINVAL),
    };
    let effective_uid = match parse_optional_u32_id(args.arg(1)) {
        Some(uid) => uid.map(Uid),
        None => return UserRet::from_error(ErrNo::EINVAL),
    };
    cred::set_reuid(real_uid, effective_uid);
    UserRet::from_success(0)
}

// 本方法代码由AI完成
pub(crate) fn sys_setregid(args: SyscallArgs) -> UserRet {
    let real_gid = match parse_optional_u32_id(args.arg(0)) {
        Some(gid) => gid.map(Gid),
        None => return UserRet::from_error(ErrNo::EINVAL),
    };
    let effective_gid = match parse_optional_u32_id(args.arg(1)) {
        Some(gid) => gid.map(Gid),
        None => return UserRet::from_error(ErrNo::EINVAL),
    };
    cred::set_regid(real_gid, effective_gid);
    UserRet::from_success(0)
}

// 本方法代码由AI完成
pub(crate) fn sys_setresuid(args: SyscallArgs) -> UserRet {
    let real_uid = match parse_optional_u32_id(args.arg(0)) {
        Some(uid) => uid.map(Uid),
        None => return UserRet::from_error(ErrNo::EINVAL),
    };
    let effective_uid = match parse_optional_u32_id(args.arg(1)) {
        Some(uid) => uid.map(Uid),
        None => return UserRet::from_error(ErrNo::EINVAL),
    };
    let saved_uid = match parse_optional_u32_id(args.arg(2)) {
        Some(uid) => uid.map(Uid),
        None => return UserRet::from_error(ErrNo::EINVAL),
    };
    cred::set_resuid(real_uid, effective_uid, saved_uid);
    UserRet::from_success(0)
}

// 本方法代码由AI完成
pub(crate) fn sys_setresgid(args: SyscallArgs) -> UserRet {
    let real_gid = match parse_optional_u32_id(args.arg(0)) {
        Some(gid) => gid.map(Gid),
        None => return UserRet::from_error(ErrNo::EINVAL),
    };
    let effective_gid = match parse_optional_u32_id(args.arg(1)) {
        Some(gid) => gid.map(Gid),
        None => return UserRet::from_error(ErrNo::EINVAL),
    };
    let saved_gid = match parse_optional_u32_id(args.arg(2)) {
        Some(gid) => gid.map(Gid),
        None => return UserRet::from_error(ErrNo::EINVAL),
    };
    cred::set_resgid(real_gid, effective_gid, saved_gid);
    UserRet::from_success(0)
}

// 本方法代码由AI完成
pub(crate) fn sys_getresuid(args: SyscallArgs) -> UserRet {
    let ruid_ptr = args.arg(0);
    let euid_ptr = args.arg(1);
    let suid_ptr = args.arg(2);
    if ruid_ptr == 0 && euid_ptr == 0 && suid_ptr == 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let cred = cred::current_credentials();
    if ruid_ptr != 0 {
        if copy_to_user_struct(ruid_ptr, &cred.real_uid.0).is_err() {
            return UserRet::from_error(ErrNo::EFAULT);
        }
    }
    if euid_ptr != 0 {
        if copy_to_user_struct(euid_ptr, &cred.effective_uid.0).is_err() {
            return UserRet::from_error(ErrNo::EFAULT);
        }
    }
    if suid_ptr != 0 {
        if copy_to_user_struct(suid_ptr, &cred.saved_uid.0).is_err() {
            return UserRet::from_error(ErrNo::EFAULT);
        }
    }
    UserRet::from_success(0)
}

// 本方法代码由AI完成
pub(crate) fn sys_getresgid(args: SyscallArgs) -> UserRet {
    let rgid_ptr = args.arg(0);
    let egid_ptr = args.arg(1);
    let sgid_ptr = args.arg(2);
    if rgid_ptr == 0 && egid_ptr == 0 && sgid_ptr == 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let cred = cred::current_credentials();
    if rgid_ptr != 0 {
        if copy_to_user_struct(rgid_ptr, &cred.real_gid.0).is_err() {
            return UserRet::from_error(ErrNo::EFAULT);
        }
    }
    if egid_ptr != 0 {
        if copy_to_user_struct(egid_ptr, &cred.effective_gid.0).is_err() {
            return UserRet::from_error(ErrNo::EFAULT);
        }
    }
    if sgid_ptr != 0 {
        if copy_to_user_struct(sgid_ptr, &cred.saved_gid.0).is_err() {
            return UserRet::from_error(ErrNo::EFAULT);
        }
    }
    UserRet::from_success(0)
}

fn parse_required_u32_id(raw: usize) -> Option<u32> {
    if raw <= u32::MAX as usize {
        Some(raw as u32)
    } else {
        None
    }
}

fn parse_optional_u32_id(raw: usize) -> Option<Option<u32>> {
    if raw == usize::MAX || raw == u32::MAX as usize {
        Some(None)
    } else {
        parse_required_u32_id(raw).map(Some)
    }
}

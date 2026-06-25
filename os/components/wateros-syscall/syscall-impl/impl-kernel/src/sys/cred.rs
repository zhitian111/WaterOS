//! 进程凭证相关系统调用：`getuid`/`geteuid`/`getgid`/`getegid`/`getgroups` 与 set*id。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use cred::api::{Gid, Uid, SUPPLEMENTARY_GROUP_COUNT};

use crate::user_copy::copy_to_user;

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

pub(crate) fn sys_setuid(args: SyscallArgs) -> UserRet {
    let uid = match parse_required_u32_id(args.arg(0)) {
        Some(uid) => Uid(uid),
        None => return UserRet::from_error(ErrNo::EINVAL),
    };
    cred::set_uid(uid);
    UserRet::from_success(0)
}

pub(crate) fn sys_setgid(args: SyscallArgs) -> UserRet {
    let gid = match parse_required_u32_id(args.arg(0)) {
        Some(gid) => Gid(gid),
        None => return UserRet::from_error(ErrNo::EINVAL),
    };
    cred::set_gid(gid);
    UserRet::from_success(0)
}

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

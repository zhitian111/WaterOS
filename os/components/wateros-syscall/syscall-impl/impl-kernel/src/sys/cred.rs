//! 进程凭证相关系统调用：`getuid`/`geteuid`/`getgid`/`getegid`/`getgroups` 与 set* panic 桩。

use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use cred::api::SUPPLEMENTARY_GROUP_COUNT;

use crate::unsupported;
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

pub(crate) fn sys_getgroups(args : SyscallArgs) -> UserRet {
    let size = args.arg(0) as isize;
    let list_ptr = args.arg(1);

    if size < 0 {
        unsupported::syscall_unsupported("getgroups: negative size");
    }

    let cred = cred::current_credentials();
    let ngroups = cred.supplementary_group_count() as usize;

    if size == 0 {
        return UserRet::from_success(ngroups);
    }

    if list_ptr == 0 {
        unsupported::syscall_unsupported("getgroups: null list pointer with size > 0");
    }

    let mut gid_buf = [0u32; SUPPLEMENTARY_GROUP_COUNT];
    for (i, g) in cred.supplementary_groups.iter().enumerate() {
        gid_buf[i] = g.0;
    }
    let bytes = unsafe {
        core::slice::from_raw_parts(gid_buf.as_ptr() as *const u8,
                                    gid_buf.len() * core::mem::size_of::<u32>())
    };
    let write_len = (size as usize).min(gid_buf.len());
    if copy_to_user(list_ptr, &bytes[..write_len * core::mem::size_of::<u32>()]).is_err() {
        unsupported::syscall_unsupported("getgroups: copy_to_user failed");
    }
    UserRet::from_success(ngroups)
}

pub(crate) fn sys_setuid(_args : SyscallArgs) -> ! {
    unsupported::syscall_unsupported("setuid");
}

pub(crate) fn sys_setgid(_args : SyscallArgs) -> ! {
    unsupported::syscall_unsupported("setgid");
}

pub(crate) fn sys_setreuid(_args : SyscallArgs) -> ! {
    unsupported::syscall_unsupported("setreuid");
}

pub(crate) fn sys_setregid(_args : SyscallArgs) -> ! {
    unsupported::syscall_unsupported("setregid");
}

pub(crate) fn sys_setresuid(_args : SyscallArgs) -> ! {
    unsupported::syscall_unsupported("setresuid");
}

pub(crate) fn sys_setresgid(_args : SyscallArgs) -> ! {
    unsupported::syscall_unsupported("setresgid");
}

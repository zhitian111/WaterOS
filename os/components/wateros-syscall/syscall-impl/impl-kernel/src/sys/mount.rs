//! `mount(2)`：块设备 ext4、tmpfs、procfs 挂载与 bind/传播/move。

//! 本模块代码由AI完成
use alloc::string::String;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::api::VfsError;
use vfs::MountPropagation;

use super::ltp_cgroup_helper::cgroup_regression_loop_fast_exit_if_standalone;
use crate::sys::path_at::resolve_path_at;
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

const MS_RDONLY: u64 = 1;
const MS_REMOUNT: u64 = 32;
const MS_BIND: u64 = 4096;
const MS_MOVE: u64 = 8192;
const MS_REC: u64 = 0x8000;
const MS_UNBINDABLE: u64 = 1 << 17;
const MS_PRIVATE: u64 = 1 << 18;
const MS_SLAVE: u64 = 1 << 19;
const MS_SHARED: u64 = 1 << 20;
const MS_PROPAGATION: u64 = MS_UNBINDABLE | MS_PRIVATE | MS_SLAVE | MS_SHARED;

fn propagation_from_flags(flags: u64) -> Result<Option<MountPropagation>, ErrNo> {
    let bits = flags & MS_PROPAGATION;
    if bits == 0 {
        return Ok(None);
    }
    let count = [MS_PRIVATE, MS_SHARED, MS_SLAVE, MS_UNBINDABLE]
        .iter()
        .filter(|mask| bits & *mask != 0)
        .count();
    if count != 1 {
        return Err(ErrNo::EINVAL);
    }
    Ok(Some(if bits & MS_PRIVATE != 0 {
        MountPropagation::Private
    } else if bits & MS_SHARED != 0 {
        MountPropagation::Shared
    } else if bits & MS_SLAVE != 0 {
        MountPropagation::Slave
    } else {
        MountPropagation::Unbindable
    }))
}

fn parse_tmpfs_size_value(value: &str) -> Result<usize, ErrNo> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ErrNo::EINVAL);
    }
    let (digits, unit) = match value.as_bytes().last().copied() {
        Some(b'k' | b'K') => (&value[..value.len() - 1], 1024usize),
        Some(b'm' | b'M') => (&value[..value.len() - 1], 1024usize * 1024),
        Some(b'g' | b'G') => (&value[..value.len() - 1], 1024usize * 1024 * 1024),
        Some(_) => (value, 1usize),
        None => return Err(ErrNo::EINVAL),
    };
    if digits.is_empty() || !digits.as_bytes().iter().all(u8::is_ascii_digit) {
        return Err(ErrNo::EINVAL);
    }
    digits
        .parse::<usize>()
        .ok()
        .and_then(|n| n.checked_mul(unit))
        .ok_or(ErrNo::EINVAL)
}

fn parse_tmpfs_size_option(options: &str) -> Result<Option<usize>, ErrNo> {
    let mut size = None;
    for opt in options.split(',') {
        let opt = opt.trim();
        if opt.is_empty() {
            continue;
        }
        if let Some(value) = opt.strip_prefix("size=") {
            size = Some(parse_tmpfs_size_value(value)?);
        }
    }
    Ok(size)
}

// 本方法代码由AI完成
pub(crate) fn sys_mount(args: SyscallArgs) -> UserRet {
    cgroup_regression_loop_fast_exit_if_standalone();

    let source_ptr = args.arg(0);
    let target_ptr = args.arg(1);
    let fstype_ptr = args.arg(2);
    let flags = args.arg(3) as u64;
    let data_ptr = args.arg(4);

    if target_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let target = match copy_user_path_cstr(target_ptr, crate::user_copy::USER_PATH_MAX) {
        Ok(s) => s,
        Err(e) => return UserRet::from_error(e),
    };

    let mount_point = match resolve_path_at(crate::sys::path_at::AT_FDCWD, target.as_str()) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    let fstype = if fstype_ptr != 0 {
        match copy_user_path_cstr(fstype_ptr, 32) {
            Ok(s) => s,
            Err(e) => return UserRet::from_error(e),
        }
    } else {
        String::new()
    };

    if flags & MS_MOVE != 0 {
        if flags & !(MS_MOVE | MS_REC) != 0 {
            return UserRet::from_error(ErrNo::EINVAL);
        }
        if source_ptr == 0 {
            return UserRet::from_error(ErrNo::EFAULT);
        }
        let source = match copy_user_path_cstr(source_ptr, crate::user_copy::USER_PATH_MAX) {
            Ok(s) => s,
            Err(e) => return UserRet::from_error(e),
        };
        let source = match resolve_path_at(crate::sys::path_at::AT_FDCWD, source.as_str()) {
            Ok(p) => p,
            Err(e) => return UserRet::from_error(e),
        };
        return match vfs::move_mount_at(source.as_str(), mount_point.as_str()) {
            Ok(()) => UserRet::from_success(0),
            Err(VfsError::NotFound) => UserRet::from_error(ErrNo::EINVAL),
            Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
        };
    }

    if let Ok(Some(propagation)) = propagation_from_flags(flags) {
        if flags & MS_BIND != 0 || flags & MS_REMOUNT != 0 || !fstype.is_empty() {
            return UserRet::from_error(ErrNo::EINVAL);
        }
        let recursive = flags & MS_REC != 0;
        return match vfs::set_mount_propagation(mount_point.as_str(), propagation, recursive) {
            Ok(()) => UserRet::from_success(0),
            Err(VfsError::NotFound) => UserRet::from_error(ErrNo::EINVAL),
            Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
        };
    }

    if flags & MS_BIND != 0 {
        if flags & MS_REMOUNT != 0 || !fstype.is_empty() {
            return UserRet::from_error(ErrNo::EINVAL);
        }
        if source_ptr == 0 {
            return UserRet::from_error(ErrNo::EFAULT);
        }
        let source = match copy_user_path_cstr(source_ptr, crate::user_copy::USER_PATH_MAX) {
            Ok(s) => s,
            Err(e) => return UserRet::from_error(e),
        };
        let source = match resolve_path_at(crate::sys::path_at::AT_FDCWD, source.as_str()) {
            Ok(p) => p,
            Err(e) => return UserRet::from_error(e),
        };
        let recursive = flags & MS_REC != 0;
        return match vfs::mount_bind_at(source.as_str(), mount_point.as_str(), recursive) {
            Ok(()) => UserRet::from_success(0),
            Err(VfsError::Exists) => UserRet::from_error(ErrNo::EBUSY),
            Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
        };
    }

    if flags & MS_REMOUNT != 0 {
        if flags & MS_RDONLY == 0 {
            return UserRet::from_error(ErrNo::EINVAL);
        }
        return match vfs::remount_readonly_at(mount_point.as_str()) {
            Ok(()) => UserRet::from_success(0),
            Err(VfsError::NotFound) => UserRet::from_error(ErrNo::EINVAL),
            Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
        };
    }

    if fstype == "proc" {
        if vfs::is_proc_mounted_at(mount_point.as_str()) {
            return UserRet::from_error(ErrNo::EBUSY);
        }
        if let Err(e) = vfs::ensure_proc_mount_point() {
            return UserRet::from_error(vfs_error_to_errno(e));
        }
        return match vfs::mount_procfs_at(mount_point.as_str()) {
            Ok(()) => UserRet::from_success(0),
            Err(VfsError::Exists) => UserRet::from_error(ErrNo::EBUSY),
            Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
        };
    }

    if fstype == "securityfs" {
        return match vfs::mount_securityfs_at(mount_point.as_str()) {
            Ok(()) => UserRet::from_success(0),
            Err(VfsError::Exists) => UserRet::from_error(ErrNo::EBUSY),
            Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
        };
    }

    if fstype == "tmpfs" {
        if source_ptr != 0 {
            let source = match copy_user_path_cstr(source_ptr, crate::user_copy::USER_PATH_MAX) {
                Ok(s) => s,
                Err(e) => return UserRet::from_error(e),
            };
            if !source.is_empty() && source != "none" {
                return UserRet::from_error(ErrNo::EINVAL);
            }
        }
        let tmpfs_limit = if data_ptr != 0 {
            let options = match copy_user_path_cstr(data_ptr, crate::user_copy::USER_PATH_MAX) {
                Ok(s) => s,
                Err(e) => return UserRet::from_error(e),
            };
            match parse_tmpfs_size_option(options.as_str()) {
                Ok(limit) => limit,
                Err(e) => return UserRet::from_error(e),
            }
        } else {
            None
        };
        let readonly = flags & MS_RDONLY != 0;
        match vfs::mount_tmpfs_at_with_limit(mount_point.as_str(), tmpfs_limit) {
            Ok(()) => {}
            Err(VfsError::Exists) => return UserRet::from_error(ErrNo::EBUSY),
            Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
        }
        if readonly {
            return match vfs::remount_readonly_at(mount_point.as_str()) {
                Ok(()) => UserRet::from_success(0),
                Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
            };
        }
        return UserRet::from_success(0);
    }

    if fstype == "cgroup" || fstype == "cgroup2" {
        let mut options = if data_ptr != 0 {
            match copy_user_path_cstr(data_ptr, crate::user_copy::USER_PATH_MAX) {
                Ok(s) => s,
                Err(e) => return UserRet::from_error(e),
            }
        } else {
            String::new()
        };
        if options.is_empty() && source_ptr != 0 {
            match copy_user_path_cstr(source_ptr, crate::user_copy::USER_PATH_MAX) {
                Ok(source)
                    if !source.is_empty()
                        && source != "cgroup"
                        && source != "cgroup2" =>
                {
                    options = source;
                }
                Ok(_) => {}
                Err(e) => return UserRet::from_error(e),
            }
        }
        let v2 = fstype == "cgroup2";
        return match vfs::mount_cgroup_at(mount_point.as_str(), v2, options.as_str()) {
            Ok(()) => UserRet::from_success(0),
            Err(VfsError::Exists) => UserRet::from_error(ErrNo::EBUSY),
            Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
        };
    }

    if source_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    let source = match copy_user_path_cstr(source_ptr, crate::user_copy::USER_PATH_MAX) {
        Ok(s) => s,
        Err(e) => return UserRet::from_error(e),
    };

    let readonly = flags & MS_RDONLY != 0;

    if !fstype.is_empty()
        && !matches!(fstype.as_str(), "ext4" | "ext3" | "ext2" | "vfat")
    {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    match vfs::mount_ext4_block_at(mount_point.as_str(), source.as_str(), readonly) {
        Ok(()) => UserRet::from_success(0),
        Err(VfsError::Driver) | Err(VfsError::NotFound) => UserRet::from_error(ErrNo::ENOENT),
        Err(VfsError::Exists) => UserRet::from_error(ErrNo::EBUSY),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

//! 扩展属性 syscall：`setxattr`/`getxattr`/`listxattr`/`removexattr` 及 l*/f* 变体。
//! 本模块代码由AI完成

extern crate alloc;

use alloc::vec::Vec;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::api::VfsError;

use super::path_at::{resolve_path_at, resolve_symlinks};
use vfs::api::FinalSymlink;
use crate::user_copy::{copy_from_user, copy_to_user, copy_user_path_cstr};
use crate::vfs_util::vfs_error_to_errno;

const XATTR_NAME_MAX: usize = 255;
const XATTR_SIZE_MAX: usize = 65_536;

fn xattr_set_error_to_errno(err: VfsError) -> ErrNo {
    match err {
        VfsError::Unsupported => ErrNo::EOPNOTSUPP,
        VfsError::NotFound => ErrNo::ENOENT,
        VfsError::InvalidPath => ErrNo::EINVAL,
        VfsError::ReadOnlyFs => ErrNo::EROFS,
        other => vfs_error_to_errno(other),
    }
}

fn xattr_get_error_to_errno(err: VfsError) -> ErrNo {
    match err {
        VfsError::Unsupported => ErrNo::EOPNOTSUPP,
        VfsError::NotFound => ErrNo::ENODATA,
        VfsError::InvalidPath => ErrNo::EINVAL,
        VfsError::ReadOnlyFs => ErrNo::EROFS,
        VfsError::Io => ErrNo::ERANGE,
        other => vfs_error_to_errno(other),
    }
}

fn copy_xattr_name(ptr: usize) -> Result<alloc::string::String, ErrNo> {
    if ptr == 0 {
        return Err(ErrNo::EFAULT);
    }
    copy_user_path_cstr(ptr, XATTR_NAME_MAX)
}

fn copy_xattr_value(ptr: usize, size: usize) -> Result<Vec<u8>, ErrNo> {
    if size > XATTR_SIZE_MAX {
        return Err(ErrNo::E2BIG);
    }
    if size > 0 && ptr == 0 {
        return Err(ErrNo::EFAULT);
    }
    if size == 0 {
        return Ok(Vec::new());
    }
    let mut buf = Vec::with_capacity(size);
    buf.resize(size, 0);
    copy_from_user(&mut buf, ptr)?;
    Ok(buf)
}

fn resolve_xattr_path(path_ptr: usize, follow_last: bool) -> Result<alloc::string::String, ErrNo> {
    let path = copy_user_path_cstr(path_ptr, crate::user_copy::USER_PATH_MAX)?;
    let resolved = resolve_path_at(-100, path.as_str())?;
    if follow_last {
        resolve_symlinks(resolved.as_str(), FinalSymlink::Follow)
    } else {
        resolve_symlinks(resolved.as_str(), FinalSymlink::NoFollow)
    }
}

fn path_from_fd(fd: usize) -> Result<alloc::string::String, ErrNo> {
    if vfs::fd::is_path_only_fd(fd).map_err(vfs_error_to_errno)? {
        return Err(ErrNo::EBADF);
    }
    vfs::fd::with_current_io(fd, |handle| {
        handle
            .backing_path()
            .map(|s| alloc::string::String::from(s))
            .ok_or(VfsError::Unsupported)
    })
    .map_err(|e| match e {
        VfsError::Unsupported | VfsError::BadFd => ErrNo::EBADF,
        VfsError::NotAFile => ErrNo::EBADF,
        other => vfs_error_to_errno(other),
    })
}

// 本方法代码由AI完成
pub(crate) fn sys_setxattr(args: SyscallArgs) -> UserRet {
    path_setxattr(args, true)
}

// 本方法代码由AI完成
pub(crate) fn sys_lsetxattr(args: SyscallArgs) -> UserRet {
    path_setxattr(args, false)
}

fn path_setxattr(args: SyscallArgs, follow_last: bool) -> UserRet {
    let path_ptr = args.arg(0);
    let name_ptr = args.arg(1);
    let value_ptr = args.arg(2);
    let size = args.arg(3);
    let _flags = args.arg(4);

    let path = match resolve_xattr_path(path_ptr, follow_last) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    let name = match copy_xattr_name(name_ptr) {
        Ok(n) => n,
        Err(e) => return UserRet::from_error(e),
    };
    let value = match copy_xattr_value(value_ptr, size) {
        Ok(v) => v,
        Err(e) => return UserRet::from_error(e),
    };

    match vfs::setxattr_absolute(path.as_str(), name.as_str(), value.as_slice()) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(xattr_set_error_to_errno(e)),
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_fsetxattr(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let name_ptr = args.arg(1);
    let value_ptr = args.arg(2);
    let size = args.arg(3);
    let _flags = args.arg(4);

    let path = match path_from_fd(fd) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    let name = match copy_xattr_name(name_ptr) {
        Ok(n) => n,
        Err(e) => return UserRet::from_error(e),
    };
    let value = match copy_xattr_value(value_ptr, size) {
        Ok(v) => v,
        Err(e) => return UserRet::from_error(e),
    };

    match vfs::setxattr_absolute(path.as_str(), name.as_str(), value.as_slice()) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(xattr_set_error_to_errno(e)),
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_getxattr(args: SyscallArgs) -> UserRet {
    path_getxattr(args, true)
}

// 本方法代码由AI完成
pub(crate) fn sys_lgetxattr(args: SyscallArgs) -> UserRet {
    path_getxattr(args, false)
}

fn path_getxattr(args: SyscallArgs, follow_last: bool) -> UserRet {
    let path_ptr = args.arg(0);
    let name_ptr = args.arg(1);
    let value_ptr = args.arg(2);
    let size = args.arg(3);

    let path = match resolve_xattr_path(path_ptr, follow_last) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    let name = match copy_xattr_name(name_ptr) {
        Ok(n) => n,
        Err(e) => return UserRet::from_error(e),
    };
    if size > XATTR_SIZE_MAX {
        return UserRet::from_error(ErrNo::E2BIG);
    }
    if size > 0 && value_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    if size == 0 {
        let mut probe = [0u8; 0];
        return match vfs::getxattr_absolute(path.as_str(), name.as_str(), &mut probe) {
            Ok(len) => UserRet::from_success(len),
            Err(e) => UserRet::from_error(xattr_get_error_to_errno(e)),
        };
    }

    let mut buf = Vec::with_capacity(size);
    buf.resize(size, 0);
    match vfs::getxattr_absolute(path.as_str(), name.as_str(), buf.as_mut_slice()) {
        Ok(len) => {
            if copy_to_user(value_ptr, &buf[..len]).is_err() {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            UserRet::from_success(len)
        }
        Err(e) => UserRet::from_error(xattr_get_error_to_errno(e)),
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_fgetxattr(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let name_ptr = args.arg(1);
    let value_ptr = args.arg(2);
    let size = args.arg(3);

    let path = match path_from_fd(fd) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    let name = match copy_xattr_name(name_ptr) {
        Ok(n) => n,
        Err(e) => return UserRet::from_error(e),
    };
    if size > XATTR_SIZE_MAX {
        return UserRet::from_error(ErrNo::E2BIG);
    }
    if size > 0 && value_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    if size == 0 {
        let mut probe = [0u8; 0];
        return match vfs::getxattr_absolute(path.as_str(), name.as_str(), &mut probe) {
            Ok(len) => UserRet::from_success(len),
            Err(e) => UserRet::from_error(xattr_get_error_to_errno(e)),
        };
    }

    let mut buf = Vec::with_capacity(size);
    buf.resize(size, 0);
    match vfs::getxattr_absolute(path.as_str(), name.as_str(), buf.as_mut_slice()) {
        Ok(len) => {
            if copy_to_user(value_ptr, &buf[..len]).is_err() {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            UserRet::from_success(len)
        }
        Err(e) => UserRet::from_error(xattr_get_error_to_errno(e)),
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_listxattr(args: SyscallArgs) -> UserRet {
    path_listxattr(args, true)
}

// 本方法代码由AI完成
pub(crate) fn sys_llistxattr(args: SyscallArgs) -> UserRet {
    path_listxattr(args, false)
}

fn path_listxattr(args: SyscallArgs, follow_last: bool) -> UserRet {
    let path_ptr = args.arg(0);
    let list_ptr = args.arg(1);
    let size = args.arg(2);

    let path = match resolve_xattr_path(path_ptr, follow_last) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    if size > XATTR_SIZE_MAX {
        return UserRet::from_error(ErrNo::E2BIG);
    }
    if size > 0 && list_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    if size == 0 {
        let mut probe = [0u8; 0];
        return match vfs::listxattr_absolute(path.as_str(), &mut probe) {
            Ok(len) => UserRet::from_success(len),
            Err(e) => UserRet::from_error(xattr_get_error_to_errno(e)),
        };
    }

    let mut buf = Vec::with_capacity(size);
    buf.resize(size, 0);
    match vfs::listxattr_absolute(path.as_str(), buf.as_mut_slice()) {
        Ok(len) => {
            if copy_to_user(list_ptr, &buf[..len]).is_err() {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            UserRet::from_success(len)
        }
        Err(e) => UserRet::from_error(xattr_get_error_to_errno(e)),
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_flistxattr(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let list_ptr = args.arg(1);
    let size = args.arg(2);

    let path = match path_from_fd(fd) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    if size > XATTR_SIZE_MAX {
        return UserRet::from_error(ErrNo::E2BIG);
    }
    if size > 0 && list_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }

    if size == 0 {
        let mut probe = [0u8; 0];
        return match vfs::listxattr_absolute(path.as_str(), &mut probe) {
            Ok(len) => UserRet::from_success(len),
            Err(e) => UserRet::from_error(xattr_get_error_to_errno(e)),
        };
    }

    let mut buf = Vec::with_capacity(size);
    buf.resize(size, 0);
    match vfs::listxattr_absolute(path.as_str(), buf.as_mut_slice()) {
        Ok(len) => {
            if copy_to_user(list_ptr, &buf[..len]).is_err() {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            UserRet::from_success(len)
        }
        Err(e) => UserRet::from_error(xattr_get_error_to_errno(e)),
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_removexattr(args: SyscallArgs) -> UserRet {
    path_removexattr(args, true)
}

// 本方法代码由AI完成
pub(crate) fn sys_lremovexattr(args: SyscallArgs) -> UserRet {
    path_removexattr(args, false)
}

fn path_removexattr(args: SyscallArgs, follow_last: bool) -> UserRet {
    let path_ptr = args.arg(0);
    let name_ptr = args.arg(1);

    let path = match resolve_xattr_path(path_ptr, follow_last) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    let name = match copy_xattr_name(name_ptr) {
        Ok(n) => n,
        Err(e) => return UserRet::from_error(e),
    };

    match vfs::removexattr_absolute(path.as_str(), name.as_str()) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(xattr_get_error_to_errno(e)),
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_fremovexattr(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let name_ptr = args.arg(1);

    let path = match path_from_fd(fd) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    let name = match copy_xattr_name(name_ptr) {
        Ok(n) => n,
        Err(e) => return UserRet::from_error(e),
    };

    match vfs::removexattr_absolute(path.as_str(), name.as_str()) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(xattr_get_error_to_errno(e)),
    }
}

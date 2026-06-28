//! Extended attribute syscalls: path and fd variants.

extern crate alloc;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use cred::api::{Capability, ProcessCredentials};
use vfs::active_impl;
use vfs::api::{SingleRootReadView, VfsError, VfsMetadata};

use crate::sys::path_at::resolve_path_at;
use crate::user_copy::{copy_from_user, copy_to_user, copy_user_path_cstr};
use crate::vfs_util::vfs_error_to_errno;

const XATTR_NAME_MAX: usize = 255;
const XATTR_SIZE_MAX: usize = 65536;

pub(crate) fn sys_setxattr(args: SyscallArgs) -> UserRet {
    let path = match resolve_xattr_path(args.arg(0), false) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    do_setxattr(path.as_str(), args.arg(1), args.arg(2), args.arg(3))
}

pub(crate) fn sys_lsetxattr(args: SyscallArgs) -> UserRet {
    let path = match resolve_xattr_path(args.arg(0), true) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    do_setxattr(path.as_str(), args.arg(1), args.arg(2), args.arg(3))
}

pub(crate) fn sys_fsetxattr(args: SyscallArgs) -> UserRet {
    let path = match fd_backing_path(args.arg(0)) {
        Ok(p) => p,
        Err(ret) => return ret,
    };
    do_setxattr(path.as_str(), args.arg(1), args.arg(2), args.arg(3))
}

pub(crate) fn sys_getxattr(args: SyscallArgs) -> UserRet {
    let path = match resolve_xattr_path(args.arg(0), false) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    do_getxattr(path.as_str(), args.arg(1), args.arg(2), args.arg(3))
}

pub(crate) fn sys_lgetxattr(args: SyscallArgs) -> UserRet {
    let path = match resolve_xattr_path(args.arg(0), true) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    do_getxattr(path.as_str(), args.arg(1), args.arg(2), args.arg(3))
}

pub(crate) fn sys_fgetxattr(args: SyscallArgs) -> UserRet {
    let path = match fd_backing_path(args.arg(0)) {
        Ok(p) => p,
        Err(ret) => return ret,
    };
    do_getxattr(path.as_str(), args.arg(1), args.arg(2), args.arg(3))
}

pub(crate) fn sys_listxattr(args: SyscallArgs) -> UserRet {
    let path = match resolve_xattr_path(args.arg(0), false) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    do_listxattr(path.as_str(), args.arg(1), args.arg(2))
}

pub(crate) fn sys_llistxattr(args: SyscallArgs) -> UserRet {
    let path = match resolve_xattr_path(args.arg(0), true) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    do_listxattr(path.as_str(), args.arg(1), args.arg(2))
}

pub(crate) fn sys_flistxattr(args: SyscallArgs) -> UserRet {
    let path = match fd_backing_path(args.arg(0)) {
        Ok(p) => p,
        Err(ret) => return ret,
    };
    do_listxattr(path.as_str(), args.arg(1), args.arg(2))
}

pub(crate) fn sys_removexattr(args: SyscallArgs) -> UserRet {
    let path = match resolve_xattr_path(args.arg(0), false) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    do_removexattr(path.as_str(), args.arg(1))
}

pub(crate) fn sys_lremovexattr(args: SyscallArgs) -> UserRet {
    let path = match resolve_xattr_path(args.arg(0), true) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    do_removexattr(path.as_str(), args.arg(1))
}

pub(crate) fn sys_fremovexattr(args: SyscallArgs) -> UserRet {
    let path = match fd_backing_path(args.arg(0)) {
        Ok(p) => p,
        Err(ret) => return ret,
    };
    do_removexattr(path.as_str(), args.arg(1))
}

fn validate_xattr_name(name: &str) -> Result<(), ErrNo> {
    if name.is_empty() || name.len() > XATTR_NAME_MAX || name.contains('\0') {
        return Err(ErrNo::EINVAL);
    }
    Ok(())
}

fn xattr_error_to_errno(err: VfsError, missing_attr: bool) -> ErrNo {
    match err {
        VfsError::NotFound if missing_attr => ErrNo::ENODATA,
        VfsError::Io => ErrNo::ERANGE,
        other => vfs_error_to_errno(other),
    }
}

fn may_access_xattr(
    cred: &ProcessCredentials,
    meta: &VfsMetadata,
    name: &str,
    write: bool,
) -> bool {
    if cred.effective_uid.0 == 0 {
        return true;
    }
    if name.starts_with("trusted.") || name.starts_with("security.") {
        return cred::has_cap(cred, Capability::SysAdmin);
    }
    let owner = cred.fs_uid.0 == meta.uid;
    if write {
        return owner || cred::has_cap(cred, Capability::SysAdmin);
    }
    owner
}

fn resolve_xattr_path(path_ptr: usize, _nofollow: bool) -> Result<alloc::string::String, ErrNo> {
    let path = copy_user_path_cstr(path_ptr, 256)?;
    resolve_path_at(-100, path.as_str()).map_err(|e| e)
}

fn load_path_metadata(path: &str) -> Result<VfsMetadata, UserRet> {
    match active_impl::backend().metadata(path) {
        Ok(meta) => Ok(meta),
        Err(VfsError::NotAFile) => Err(UserRet::from_error(ErrNo::ENOTDIR)),
        Err(e) => Err(UserRet::from_error(vfs_error_to_errno(e))),
    }
}

fn fd_backing_path(fd: usize) -> Result<alloc::string::String, UserRet> {
    vfs::fd::with_current_io(fd, |handle| {
        handle
            .backing_path()
            .map(alloc::string::String::from)
            .ok_or(VfsError::Unsupported)
    })
    .map_err(|e| UserRet::from_error(vfs_error_to_errno(e)))
}

fn do_setxattr(path: &str, name_ptr: usize, value_ptr: usize, size: usize) -> UserRet {
    if size > XATTR_SIZE_MAX {
        return UserRet::from_error(ErrNo::E2BIG);
    }
    let name = match copy_user_path_cstr(name_ptr, XATTR_NAME_MAX + 1) {
        Ok(n) => n,
        Err(e) => return UserRet::from_error(e),
    };
    if let Err(e) = validate_xattr_name(name.as_str()) {
        return UserRet::from_error(e);
    }
    let meta = match load_path_metadata(path) {
        Ok(m) => m,
        Err(ret) => return ret,
    };
    let cred = cred::current_credentials();
    if !may_access_xattr(&cred, &meta, name.as_str(), true) {
        return UserRet::from_error(ErrNo::EPERM);
    }
    let mut value = alloc::vec::Vec::with_capacity(size);
    value.resize(size, 0);
    if size != 0 {
        if let Err(e) = copy_from_user(value.as_mut_slice(), value_ptr) {
            return UserRet::from_error(e);
        }
    }
    match vfs::setxattr_absolute(path, name.as_str(), value.as_slice()) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

fn do_getxattr(path: &str, name_ptr: usize, buf_ptr: usize, size: usize) -> UserRet {
    let name = match copy_user_path_cstr(name_ptr, XATTR_NAME_MAX + 1) {
        Ok(n) => n,
        Err(e) => return UserRet::from_error(e),
    };
    if let Err(e) = validate_xattr_name(name.as_str()) {
        return UserRet::from_error(e);
    }
    let meta = match load_path_metadata(path) {
        Ok(m) => m,
        Err(ret) => return ret,
    };
    let cred = cred::current_credentials();
    if !may_access_xattr(&cred, &meta, name.as_str(), false) {
        return UserRet::from_error(ErrNo::EPERM);
    }
    let mut buf = alloc::vec::Vec::new();
    if size == 0 {
        match vfs::getxattr_absolute(path, name.as_str(), &mut buf) {
            Ok(len) => UserRet::from_success(len),
            Err(e) => UserRet::from_error(xattr_error_to_errno(e, true)),
        }
    } else if buf_ptr == 0 {
        UserRet::from_error(ErrNo::EFAULT)
    } else {
        buf.resize(size, 0);
        match vfs::getxattr_absolute(path, name.as_str(), buf.as_mut_slice()) {
            Ok(len) => {
                if let Err(e) = copy_to_user(buf_ptr, &buf[..len]) {
                    UserRet::from_error(e)
                } else {
                    UserRet::from_success(len)
                }
            }
            Err(e) => UserRet::from_error(xattr_error_to_errno(e, true)),
        }
    }
}

fn do_listxattr(path: &str, buf_ptr: usize, size: usize) -> UserRet {
    let _meta = match load_path_metadata(path) {
        Ok(m) => m,
        Err(ret) => return ret,
    };
    let mut buf = alloc::vec::Vec::new();
    if size == 0 {
        match vfs::listxattr_absolute(path, &mut buf) {
            Ok(len) => UserRet::from_success(len),
            Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
        }
    } else if buf_ptr == 0 {
        UserRet::from_error(ErrNo::EFAULT)
    } else {
        buf.resize(size, 0);
        match vfs::listxattr_absolute(path, buf.as_mut_slice()) {
            Ok(len) => {
                if let Err(e) = copy_to_user(buf_ptr, &buf[..len]) {
                    UserRet::from_error(e)
                } else {
                    UserRet::from_success(len)
                }
            }
            Err(e) => UserRet::from_error(xattr_error_to_errno(e, false)),
        }
    }
}

fn do_removexattr(path: &str, name_ptr: usize) -> UserRet {
    let name = match copy_user_path_cstr(name_ptr, XATTR_NAME_MAX + 1) {
        Ok(n) => n,
        Err(e) => return UserRet::from_error(e),
    };
    if let Err(e) = validate_xattr_name(name.as_str()) {
        return UserRet::from_error(e);
    }
    let meta = match load_path_metadata(path) {
        Ok(m) => m,
        Err(ret) => return ret,
    };
    let cred = cred::current_credentials();
    if !may_access_xattr(&cred, &meta, name.as_str(), true) {
        return UserRet::from_error(ErrNo::EPERM);
    }
    match vfs::removexattr_absolute(path, name.as_str()) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(xattr_error_to_errno(e, true)),
    }
}

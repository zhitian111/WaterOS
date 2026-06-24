//! `acct(2)`：进程 accounting 的最小兼容入口。

extern crate alloc;

use alloc::string::String;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::active_impl;
use vfs::api::{resolve_against_cwd, SingleRootReadView, VfsError, VfsNodeType};

use crate::sys::path_at::{resolve_path_at, AT_FDCWD};
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

pub(crate) fn sys_acct(args: SyscallArgs) -> UserRet {
    match do_acct(args.arg(0)) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}

fn do_acct(path_ptr: usize) -> Result<(), ErrNo> {
    if cred::current_credentials().effective_uid.0 != 0 {
        return Err(ErrNo::EPERM);
    }
    if path_ptr == 0 {
        return Ok(());
    }

    let path = copy_user_path_cstr(path_ptr, 256)?;
    if path == "/dev/null" {
        return Err(ErrNo::EACCES);
    }
    let resolved = resolve_path_at(AT_FDCWD, path.as_str())?;
    let resolved = resolve_final_symlink(resolved.as_str())?;
    if path.ends_with('/') {
        match active_impl::backend().metadata(resolved.as_str()) {
            Ok(meta) if meta.node_type != VfsNodeType::Directory => return Err(ErrNo::ENOTDIR),
            Ok(_) => {}
            Err(VfsError::NotAFile) => return Err(ErrNo::ENOTDIR),
            Err(e) => return Err(vfs_error_to_errno(e)),
        }
    }

    if let Err(e) = vfs::assert_path_writable(resolved.as_str()) {
        return Err(vfs_error_to_errno(e));
    }

    match active_impl::backend().metadata(resolved.as_str()) {
        Ok(meta) => match meta.node_type {
            VfsNodeType::File => Ok(()),
            VfsNodeType::Directory => Err(ErrNo::EISDIR),
            VfsNodeType::Symlink => Err(ErrNo::ELOOP),
            VfsNodeType::Special => Err(ErrNo::EACCES),
        },
        Err(VfsError::NotAFile) => Err(ErrNo::ENOTDIR),
        Err(e) => Err(vfs_error_to_errno(e)),
    }
}

fn resolve_final_symlink(path: &str) -> Result<String, ErrNo> {
    let mut current = String::from(path);
    for _ in 0..40 {
        let target = match vfs::read_symlink_absolute(current.as_str()) {
            Ok(target) => target,
            Err(VfsError::NotAFile) => return Ok(current),
            Err(e) => return Err(vfs_error_to_errno(e)),
        };
        let target = core::str::from_utf8(target.as_slice()).map_err(|_| ErrNo::EINVAL)?;
        let parent = current
            .rsplit_once('/')
            .map(|(parent, _)| if parent.is_empty() { "/" } else { parent })
            .unwrap_or("/");
        current = resolve_against_cwd(parent, Some(target)).map_err(vfs_error_to_errno)?;
    }
    Err(ErrNo::ELOOP)
}

//! `renameat2(2)`：bring-up 最小兼容实现，暂非原子 rename。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::active_impl;
use vfs::api::{SingleRootReadView, VfsError, VfsNodeType, VfsOpenFlags, VfsOpenOps};

use crate::sys::path_at::resolve_path_at;
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

pub(crate) fn sys_renameat2(args: SyscallArgs) -> UserRet {
    let old_dirfd = args.arg(0) as isize;
    let old_path_ptr = args.arg(1);
    let new_dirfd = args.arg(2) as isize;
    let new_path_ptr = args.arg(3);
    let flags = args.arg(4) as u32;

    if flags != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let old_path = match copy_user_path_cstr(old_path_ptr, 256) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    let new_path = match copy_user_path_cstr(new_path_ptr, 256) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    let old_resolved = match resolve_path_at(old_dirfd, old_path.as_str()) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    let new_resolved = match resolve_path_at(new_dirfd, new_path.as_str()) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    if old_resolved == new_resolved {
        return UserRet::from_success(0);
    }

    match rename_regular_file(old_resolved.as_str(), new_resolved.as_str()) {
        Ok(()) => UserRet::from_success(0),
        Err(e) => UserRet::from_error(e),
    }
}

fn rename_regular_file(old_path: &str, new_path: &str) -> Result<(), ErrNo> {
    let backend = active_impl::backend();
    let meta = backend
        .metadata(old_path)
        .map_err(vfs_error_to_errno)?;
    if meta.node_type != VfsNodeType::File {
        return Err(ErrNo::ENOSYS);
    }

    let data = backend.read(old_path).map_err(vfs_error_to_errno)?;
    let mut new_handle = backend
        .open(
            new_path,
            VfsOpenFlags(VfsOpenFlags::WRITE | VfsOpenFlags::CREATE | VfsOpenFlags::TRUNC),
        )
        .map_err(vfs_error_to_errno)?;
    let mut written = 0usize;
    while written < data.len() {
        let n = new_handle
            .write(&data[written..])
            .map_err(vfs_error_to_errno)?;
        if n == 0 {
            return Err(ErrNo::EIO);
        }
        written += n;
    }
    new_handle.close().map_err(vfs_error_to_errno)?;

    match vfs::unlink_absolute(old_path, false) {
        Ok(()) => Ok(()),
        Err(VfsError::NotAFile) => Err(ErrNo::EISDIR),
        Err(e) => Err(vfs_error_to_errno(e)),
    }
}

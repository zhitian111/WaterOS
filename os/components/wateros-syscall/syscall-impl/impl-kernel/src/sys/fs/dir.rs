use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use cred::api::{Gid, ProcessCredentials};
use vfs::active_impl;
use vfs::api::{VfsError, VfsMetadata, VfsNodeType};
use vfs::SingleRootReadView;

use crate::sys::path_at::{resolve_path_at, resolve_symlinks, AT_REMOVEDIR};
use crate::user_copy::{copy_to_user, copy_user_path_cstr};
use crate::vfs_util::vfs_error_to_errno;
use vfs::api::FinalSymlink;

const S_IFMT : u32 = 0o170_000;
const S_IFREG : u32 = 0o100_000;
const S_IFIFO : u32 = 0o10_000;
const S_IFCHR : u32 = 0o20_000;
const S_IFBLK : u32 = 0o60_000;
const S_IFSOCK : u32 = 0o140_000;
const AT_SYMLINK_FOLLOW : u32 = 0x400;
const AT_EMPTY_PATH : u32 = 0x1000;

pub(crate) fn sys_mkdirat(args : SyscallArgs) -> UserRet {
    let dirfd = args.arg(0) as isize;
    let path_ptr = args.arg(1);
    let mode = args.arg(2) as u32;

    let path = match copy_user_path_cstr(path_ptr,
                                         crate::user_copy::USER_PATH_MAX)
    {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    let resolved = match resolve_path_at(dirfd, path.as_str()) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    let resolved = match resolve_symlinks(resolved.as_str(),
                                          FinalSymlink::NoFollow)
    {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    let cred = cred::current_credentials();
    let parent_meta = match check_parent_create(resolved.as_str(), &cred) {
        Ok(meta) => meta,
        Err(e) => return UserRet::from_error(e),
    };
    let mut create_mode = mode & !crate::sys::task::current_umask() & 0o7777;
    let gid = if parent_meta.mode & 0o2000 != 0 {
        create_mode |= 0o2000;
        parent_meta.gid
    } else {
        cred.effective_gid.0
    };

    match vfs::mkdir_absolute(resolved.as_str(), create_mode) {
        Ok(()) => {
            if let Err(e) = vfs::chown_absolute(resolved.as_str(),
                                                Some(cred.effective_uid.0),
                                                Some(gid))
            {
                return UserRet::from_error(vfs_error_to_errno(e));
            }
            if let Err(e) = vfs::chmod_absolute(resolved.as_str(), create_mode) {
                return UserRet::from_error(vfs_error_to_errno(e));
            }
            super::inotify::notify_create(resolved.as_str(), true);
            UserRet::from_success(0)
        }
        Err(VfsError::NotAFile) => UserRet::from_error(ErrNo::ENOTDIR),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

fn check_parent_create(path : &str, cred : &ProcessCredentials) -> Result<VfsMetadata, ErrNo> {
    let parent = path.rsplit_once('/')
                     .map(|(parent, _)| if parent.is_empty() { "/" } else { parent })
                     .unwrap_or("/");
    let meta = match active_impl::backend().metadata(parent) {
        Ok(meta) if meta.node_type == VfsNodeType::Directory => meta,
        Ok(_) => return Err(ErrNo::ENOTDIR),
        Err(VfsError::NotFound) => return Err(ErrNo::ENOENT),
        Err(e) => return Err(vfs_error_to_errno(e)),
    };
    if can_create_in_directory(&meta, cred) {
        Ok(meta)
    } else {
        Err(ErrNo::EACCES)
    }
}

pub(crate) fn check_directory_create(path : &str,
                                     cred : &ProcessCredentials)
                                     -> Result<VfsMetadata, ErrNo> {
    let meta = match active_impl::backend().metadata(path) {
        Ok(meta) if meta.node_type == VfsNodeType::Directory => meta,
        Ok(_) => return Err(ErrNo::ENOTDIR),
        Err(error) => return Err(vfs_error_to_errno(error)),
    };
    if can_create_in_directory(&meta, cred) {
        Ok(meta)
    } else {
        Err(ErrNo::EACCES)
    }
}

fn can_create_in_directory(meta : &VfsMetadata, cred : &ProcessCredentials) -> bool {
    if cred.effective_uid.0 == 0 {
        return true;
    }
    let mode = u32::from(meta.mode);
    let required = if cred.effective_uid.0 == meta.uid {
        0o300
    } else if cred_has_group(cred, Gid(meta.gid)) {
        0o030
    } else {
        0o003
    };
    mode & required == required
}

fn cred_has_group(cred : &ProcessCredentials, gid : Gid) -> bool {
    cred.effective_gid == gid ||
    cred.supplementary_groups
        .iter()
        .any(|g| *g == gid)
}
pub(crate) fn sys_mknodat(args : SyscallArgs) -> UserRet {
    let dirfd = args.arg(0) as isize;
    let path_ptr = args.arg(1);
    let mode = args.arg(2) as u32;
    let rdev = args.arg(3) as u32;

    let path = match copy_user_path_cstr(path_ptr,
                                         crate::user_copy::USER_PATH_MAX)
    {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };
    let resolved = match resolve_path_at(dirfd, path.as_str()) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };
    let resolved = match resolve_symlinks(resolved.as_str(),
                                          FinalSymlink::NoFollow)
    {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };

    let node_type = normalize_node_type(mode);
    if !is_supported_node_type(node_type) {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let cred = cred::current_credentials();
    if (node_type == S_IFCHR || node_type == S_IFBLK) && cred.effective_uid.0 != 0 {
        return UserRet::from_error(ErrNo::EPERM);
    }
    let parent_meta = match check_parent_create(resolved.as_str(), &cred) {
        Ok(meta) => meta,
        Err(e) => return UserRet::from_error(e),
    };
    let create_perm = (mode & 0o7777) & !crate::sys::task::current_umask();
    let create_mode = node_type | create_perm;
    let gid = if parent_meta.mode & 0o2000 != 0 {
        parent_meta.gid
    } else {
        cred.effective_gid.0
    };

    match vfs::mknod_absolute(resolved.as_str(), create_mode, rdev) {
        Ok(()) => {
            if let Err(e) = vfs::chown_absolute(resolved.as_str(),
                                                Some(cred.effective_uid.0),
                                                Some(gid))
            {
                return UserRet::from_error(vfs_error_to_errno(e));
            }
            if let Err(e) = vfs::chmod_absolute(resolved.as_str(), create_perm) {
                return UserRet::from_error(vfs_error_to_errno(e));
            }
            super::inotify::notify_create(resolved.as_str(), false);
            UserRet::from_success(0)
        }
        Err(VfsError::NotAFile) => UserRet::from_error(ErrNo::ENOTDIR),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

fn normalize_node_type(mode : u32) -> u32 {
    match mode & S_IFMT {
        0 => S_IFREG,
        ty => ty,
    }
}

fn is_supported_node_type(node_type : u32) -> bool {
    matches!(node_type,
             S_IFREG | S_IFIFO | S_IFCHR | S_IFBLK | S_IFSOCK)
}
pub(crate) fn sys_linkat(args : SyscallArgs) -> UserRet {
    let old_dirfd = args.arg(0) as isize;
    let old_path_ptr = args.arg(1);
    let new_dirfd = args.arg(2) as isize;
    let new_path_ptr = args.arg(3);
    let flags = args.arg(4) as u32;

    if flags & !(AT_SYMLINK_FOLLOW | AT_EMPTY_PATH) != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let old_path = match copy_user_path_cstr(old_path_ptr,
                                             crate::user_copy::USER_PATH_MAX)
    {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };
    let new_path = match copy_user_path_cstr(new_path_ptr,
                                             crate::user_copy::USER_PATH_MAX)
    {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };

    if old_path.is_empty() {
        if flags & AT_EMPTY_PATH == 0 {
            return UserRet::from_error(ErrNo::ENOENT);
        }
        if old_dirfd < 0 {
            return UserRet::from_error(ErrNo::EBADF);
        }
        let new_resolved = match resolve_path_at(new_dirfd, new_path.as_str()) {
            Ok(path) => path,
            Err(error) => return UserRet::from_error(error),
        };
        let new_resolved = match resolve_symlinks(new_resolved.as_str(),
                                                  FinalSymlink::NoFollow)
        {
            Ok(path) => path,
            Err(error) => return UserRet::from_error(error),
        };
        let cred = cred::current_credentials();
        if let Err(error) = check_parent_create(new_resolved.as_str(), &cred) {
            return UserRet::from_error(error);
        }
        return match vfs::fd::with_current_io(old_dirfd as usize, |handle| {
                  handle.link_at_empty_path(new_resolved.as_str())
              }) {
            Ok(()) => {
                super::inotify::notify_create(new_resolved.as_str(), false);
                UserRet::from_success(0)
            }
            Err(VfsError::Exists) => UserRet::from_error(ErrNo::EEXIST),
            Err(VfsError::ReadOnlyFs) => UserRet::from_error(ErrNo::EROFS),
            Err(VfsError::Unsupported) => UserRet::from_error(ErrNo::EXDEV),
            Err(error) => UserRet::from_error(vfs_error_to_errno(error)),
        };
    }

    let old_resolved = match resolve_path_at(old_dirfd, old_path.as_str()) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };
    let old_resolved = if flags & AT_SYMLINK_FOLLOW != 0 {
        match resolve_symlinks(old_resolved.as_str(),
                               FinalSymlink::Follow)
        {
            Ok(path) => path,
            Err(e) => return UserRet::from_error(e),
        }
    } else {
        match resolve_symlinks(old_resolved.as_str(),
                               FinalSymlink::NoFollow)
        {
            Ok(path) => path,
            Err(e) => return UserRet::from_error(e),
        }
    };
    let new_resolved = match resolve_path_at(new_dirfd, new_path.as_str()) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };
    let new_resolved = match resolve_symlinks(new_resolved.as_str(),
                                              FinalSymlink::NoFollow)
    {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };
    if let Err(e) = reject_non_directory_prefix(old_resolved.as_str()) {
        return UserRet::from_error(e);
    }
    if let Err(e) = reject_non_directory_prefix(new_resolved.as_str()) {
        return UserRet::from_error(e);
    }

    match active_impl::backend().metadata(old_resolved.as_str()) {
        Ok(meta) if meta.node_type == VfsNodeType::Directory => {
            return UserRet::from_error(ErrNo::EPERM);
        }
        Ok(_) => {}
        Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
    }

    let cred = cred::current_credentials();
    if let Err(e) = check_parent_create(new_resolved.as_str(), &cred) {
        return UserRet::from_error(e);
    }

    if vfs::mount_statfs_magic(old_resolved.as_str()) !=
       vfs::mount_statfs_magic(new_resolved.as_str())
    {
        return UserRet::from_error(ErrNo::EXDEV);
    }

    match vfs::hardlink_absolute(old_resolved.as_str(),
                                 new_resolved.as_str())
    {
        Ok(()) => {
            super::inotify::notify_create(new_resolved.as_str(), false);
            UserRet::from_success(0)
        }
        Err(VfsError::Exists) => UserRet::from_error(ErrNo::EEXIST),
        Err(VfsError::ReadOnlyFs) => UserRet::from_error(ErrNo::EROFS),
        Err(VfsError::Unsupported) => UserRet::from_error(ErrNo::EXDEV),
        Err(VfsError::NotAFile) => UserRet::from_error(ErrNo::EPERM),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

fn reject_non_directory_prefix(path : &str) -> Result<(), ErrNo> {
    let trimmed = path.trim_start_matches('/');
    if trimmed.is_empty() {
        return Ok(());
    }
    let mut current = alloc::string::String::from("/");
    let mut parts = trimmed.split('/')
                           .peekable();
    while let Some(component) = parts.next() {
        if parts.peek()
                .is_none()
        {
            break;
        }
        if current != "/" {
            current.push('/');
        }
        current.push_str(component);
        match active_impl::backend().metadata(current.as_str()) {
            Ok(meta) if meta.node_type != VfsNodeType::Directory => return Err(ErrNo::ENOTDIR),
            Ok(_) => {}
            Err(VfsError::NotFound) => return Ok(()),
            Err(e) => return Err(vfs_error_to_errno(e)),
        }
    }
    Ok(())
}


pub(crate) fn sys_symlinkat(args : SyscallArgs) -> UserRet {
    let target_ptr = args.arg(0);
    let dirfd = args.arg(1) as isize;
    let linkpath_ptr = args.arg(2);

    let target = match copy_user_path_cstr(target_ptr,
                                           crate::user_copy::USER_PATH_MAX)
    {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };
    let linkpath = match copy_user_path_cstr(linkpath_ptr,
                                             crate::user_copy::USER_PATH_MAX)
    {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };

    let resolved = match resolve_path_at(dirfd, linkpath.as_str()) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };
    // 展开链接路径中的中间符号链接（如 `/lib -> usr/lib`），但保留最终待创建的
    // 链接名（NoFollow）。否则 fs 后端 `generic_lookup` 无法穿过 `/lib` 这类中间
    // 链接，会误报 NotAFile → EISDIR（Linux 的 VFS 层负责跟随中间链接）。
    let resolved = match resolve_symlinks(resolved.as_str(),
                                          FinalSymlink::NoFollow)
    {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };
    let cred = cred::current_credentials();
    if let Err(e) = check_parent_create(resolved.as_str(), &cred) {
        return UserRet::from_error(e);
    }

    match vfs::symlink_absolute(target.as_str(), resolved.as_str()) {
        Ok(()) => {
            super::inotify::notify_create(resolved.as_str(), false);
            UserRet::from_success(0)
        }
        Err(VfsError::Exists) => UserRet::from_error(ErrNo::EEXIST),
        Err(VfsError::NotFound) => UserRet::from_error(ErrNo::ENOENT),
        Err(VfsError::NotAFile) => UserRet::from_error(ErrNo::ENOTDIR),
        Err(VfsError::ReadOnlyFs) => UserRet::from_error(ErrNo::EROFS),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}


pub(crate) fn sys_unlinkat(args : SyscallArgs) -> UserRet {
    let dirfd = args.arg(0) as isize;
    let path_ptr = args.arg(1);
    let flags = args.arg(2) as u32;

    let path = match copy_user_path_cstr(path_ptr,
                                         crate::user_copy::USER_PATH_MAX)
    {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    let remove_dir = flags & AT_REMOVEDIR != 0;
    if remove_dir && (path == "." || path == "..") {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let resolved = match resolve_path_at(dirfd, path.as_str()) {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };

    let resolved = match resolve_symlinks(resolved.as_str(),
                                          FinalSymlink::NoFollow)
    {
        Ok(p) => p,
        Err(e) => return UserRet::from_error(e),
    };
    if flags & !AT_REMOVEDIR != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if let Err(e) = reject_non_directory_prefix(resolved.as_str()) {
        return UserRet::from_error(e);
    }
    if let Err(e) = check_unlink_permission(resolved.as_str()) {
        return UserRet::from_error(e);
    }
    if remove_dir && vfs::is_mount_point_absolute(resolved.as_str()) {
        return UserRet::from_error(ErrNo::EBUSY);
    }

    let is_dir = active_impl::backend().metadata(resolved.as_str())
                                       .map(|meta| meta.node_type == VfsNodeType::Directory)
                                       .unwrap_or(remove_dir);
    match vfs::unlink_absolute(resolved.as_str(), remove_dir) {
        Ok(()) => {
            super::inotify::notify_delete(resolved.as_str(), is_dir);
            UserRet::from_success(0)
        }
        Err(VfsError::NotAFile) if remove_dir => UserRet::from_error(ErrNo::ENOTDIR),
        Err(VfsError::NotAFile) => UserRet::from_error(ErrNo::EISDIR),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

fn check_unlink_permission(path : &str) -> Result<(), ErrNo> {
    let cred = cred::current_credentials();
    if cred.effective_uid.0 == 0 {
        return Ok(());
    }
    let parent = path.rsplit_once('/')
                     .map(|(parent, _)| if parent.is_empty() { "/" } else { parent })
                     .unwrap_or("/");
    let parent_meta = match active_impl::backend().metadata(parent) {
        Ok(meta) if meta.node_type == VfsNodeType::Directory => meta,
        Ok(_) => return Err(ErrNo::ENOTDIR),
        Err(VfsError::NotFound) => return Err(ErrNo::ENOENT),
        Err(e) => return Err(vfs_error_to_errno(e)),
    };
    if !can_write_search_directory(&parent_meta, &cred) {
        return Err(ErrNo::EACCES);
    }
    if parent_meta.mode & 0o1000 != 0 {
        let target_meta = match active_impl::backend().metadata(path) {
            Ok(meta) => meta,
            Err(VfsError::NotFound) => return Err(ErrNo::ENOENT),
            Err(e) => return Err(vfs_error_to_errno(e)),
        };
        if cred.effective_uid.0 != parent_meta.uid && cred.effective_uid.0 != target_meta.uid {
            return Err(ErrNo::EPERM);
        }
    }
    Ok(())
}

fn can_write_search_directory(meta : &VfsMetadata, cred : &ProcessCredentials) -> bool {
    if cred.effective_uid.0 == 0 {
        return true;
    }
    let mode = u32::from(meta.mode);
    let required = if cred.effective_uid.0 == meta.uid {
        0o300
    } else if cred_has_group(cred, Gid(meta.gid)) {
        0o030
    } else {
        0o003
    };
    mode & required == required
}

fn check_readlink_parent_search(path : &str, cred : &ProcessCredentials) -> Result<(), ErrNo> {
    if cred.effective_uid.0 == 0 {
        return Ok(());
    }
    let parts : alloc::vec::Vec<&str> = path.trim_start_matches('/')
                                            .split('/')
                                            .filter(|part| !part.is_empty())
                                            .collect();
    if parts.len() <= 1 {
        return Ok(());
    }
    let mut current = alloc::string::String::from("/");
    for part in &parts[..parts.len() - 1] {
        if current != "/" {
            current.push('/');
        }
        current.push_str(part);
        match active_impl::backend().metadata(current.as_str()) {
            Ok(meta) if meta.node_type == VfsNodeType::Directory => {
                let mode = u32::from(meta.mode);
                let execute = if cred.effective_uid.0 == meta.uid {
                    0o100
                } else if cred_has_group(cred, Gid(meta.gid)) {
                    0o010
                } else {
                    0o001
                };
                if mode & execute == 0 {
                    return Err(ErrNo::EACCES);
                }
            }
            Ok(_) => return Err(ErrNo::ENOTDIR),
            Err(VfsError::NotFound) => return Err(ErrNo::ENOENT),
            Err(e) => return Err(vfs_error_to_errno(e)),
        }
    }
    Ok(())
}

pub(crate) fn sys_readlinkat(args : SyscallArgs) -> UserRet {
    let dirfd = args.arg(0) as isize;
    let path_ptr = args.arg(1);
    let buf_ptr = args.arg(2);
    let buf_size = args.arg(3);

    if buf_size == 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let path = match copy_user_path_cstr(path_ptr,
                                         crate::user_copy::USER_PATH_MAX)
    {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };
    // Linux：`readlinkat(fd, "", …)` 读取 fd 本身指向的符号链接
    // （systemd-tmpfiles 用 `O_PATH|O_NOFOLLOW` 打开的 fd 校验链接目标）。
    let resolved = if path.is_empty() {
        if dirfd < 0 {
            return UserRet::from_error(ErrNo::EBADF);
        }
        match vfs::fd::with_current_io(dirfd as usize, |handle| {
                  handle.backing_path()
                        .map(alloc::string::String::from)
                        .ok_or(VfsError::NotAFile)
              }) {
            Ok(path) => path,
            Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
        }
    } else {
        match resolve_path_at(dirfd, path.as_str()).and_then(|path| {
                                                       resolve_symlinks(path.as_str(),
                                                                        FinalSymlink::NoFollow)
                                                   }) {
            Ok(path) => path,
            Err(e) => return UserRet::from_error(e),
        }
    };
    let cred = cred::current_credentials();
    if let Err(e) = check_readlink_parent_search(resolved.as_str(), &cred) {
        return UserRet::from_error(e);
    }
    let target = match vfs::read_symlink_absolute(resolved.as_str()) {
        Ok(target) => target,
        Err(VfsError::NotAFile) => return UserRet::from_error(ErrNo::EINVAL),
        Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
    };
    let count = core::cmp::min(buf_size, target.len());
    match copy_to_user(buf_ptr, &target[..count]) {
        Ok(_) => UserRet::from_success(count),
        Err(e) => UserRet::from_error(e),
    }
}

//! `mknodat(2)`：相对目录创建普通/特殊文件节点。
//! 本模块代码由AI完成

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use cred::api::{Gid, ProcessCredentials};
use vfs::active_impl;
use vfs::api::{SingleRootReadView, VfsError, VfsMetadata, VfsNodeType};

use crate::sys::path_at::resolve_path_at;
use crate::sys::task;
use crate::user_copy::copy_user_path_cstr;
use crate::vfs_util::vfs_error_to_errno;

const S_IFMT: u32 = 0o170000;
const S_IFIFO: u32 = 0o010000;
const S_IFCHR: u32 = 0o020000;
const S_IFBLK: u32 = 0o060000;
const S_IFREG: u32 = 0o100000;
const S_IFSOCK: u32 = 0o140000;

// 本方法代码由AI完成
pub(crate) fn sys_mknodat(args: SyscallArgs) -> UserRet {
    let dirfd = args.arg(0) as isize;
    let path_ptr = args.arg(1);
    let mode = args.arg(2) as u32;
    let rdev = args.arg(3) as u32;

    let path = match copy_user_path_cstr(path_ptr, crate::user_copy::USER_PATH_MAX) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };
    let resolved = match resolve_path_at(dirfd, path.as_str()) {
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
    let create_perm = (mode & 0o7777) & !task::current_umask();
    let create_mode = node_type | create_perm;
    let gid = if parent_meta.mode & 0o2000 != 0 {
        parent_meta.gid
    } else {
        cred.effective_gid.0
    };

    match vfs::mknod_absolute(resolved.as_str(), create_mode, rdev) {
        Ok(()) => {
            if let Err(e) =
                vfs::chown_absolute(resolved.as_str(), Some(cred.effective_uid.0), Some(gid))
            {
                return UserRet::from_error(vfs_error_to_errno(e));
            }
            if let Err(e) = vfs::chmod_absolute(resolved.as_str(), create_perm) {
                return UserRet::from_error(vfs_error_to_errno(e));
            }
            UserRet::from_success(0)
        }
        Err(VfsError::NotAFile) => UserRet::from_error(ErrNo::ENOTDIR),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

fn normalize_node_type(mode: u32) -> u32 {
    match mode & S_IFMT {
        0 => S_IFREG,
        ty => ty,
    }
}

fn is_supported_node_type(node_type: u32) -> bool {
    matches!(node_type, S_IFREG | S_IFIFO | S_IFCHR | S_IFBLK | S_IFSOCK)
}

fn check_parent_create(path: &str, cred: &ProcessCredentials) -> Result<VfsMetadata, ErrNo> {
    let parent = path
        .rsplit_once('/')
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

fn can_create_in_directory(meta: &VfsMetadata, cred: &ProcessCredentials) -> bool {
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

fn cred_has_group(cred: &ProcessCredentials, gid: Gid) -> bool {
    cred.effective_gid == gid
        || cred
            .supplementary_groups
            .iter()
            .take(cred.supplementary_group_len)
            .any(|group| *group == gid)
}

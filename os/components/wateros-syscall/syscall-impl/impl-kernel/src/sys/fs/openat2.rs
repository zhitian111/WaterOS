//! `openat2(2)`：版本化 `open_how` 校验及受约束路径打开。

extern crate alloc;

use alloc::{string::String, vec::Vec};

use api_v0::{ErrNo, SyscallArgs, UserRet};
use vfs::api::VfsError;

use super::{
    openat::{open_resolved_path, openat_path},
    path_at::{resolve_path_at, resolve_symlinks},
};
use crate::{user_copy::{copy_from_user, copy_user_path_cstr}, vfs_util::vfs_error_to_errno};

const OPEN_HOW_SIZE_VER0 : usize = 24;
const OPEN_HOW_MAX_SIZE : usize = 4096;

const RESOLVE_NO_XDEV : u64 = 0x01;
const RESOLVE_NO_MAGICLINKS : u64 = 0x02;
const RESOLVE_NO_SYMLINKS : u64 = 0x04;
const RESOLVE_BENEATH : u64 = 0x08;
const RESOLVE_IN_ROOT : u64 = 0x10;
const RESOLVE_CACHED : u64 = 0x20;
const VALID_RESOLVE : u64 = RESOLVE_NO_XDEV | RESOLVE_NO_MAGICLINKS | RESOLVE_NO_SYMLINKS |
                            RESOLVE_BENEATH | RESOLVE_IN_ROOT | RESOLVE_CACHED;

const O_CREAT : u32 = 0o100;
const O_NOFOLLOW : u32 = 0o400000;
const O_TMPFILE : u32 = 0o20200000;
const VALID_OPEN_FLAGS : u32 = 0o3 | 0o100 | 0o200 | 0o400 | 0o1000 | 0o2000 | 0o4000 |
                               0o10000 | 0o20000 | 0o40000 | 0o100000 | 0o200000 |
                               0o400000 | 0o1000000 | 0o2000000 | 0o4010000 |
                               0o10000000 | 0o20000000;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct OpenHow {
    /// 打开标志，按 Linux openat2 ABI 编码。
    flags : u64,
    /// 创建文件时使用的权限模式。
    mode : u64,
    /// 路径解析约束位图。
    resolve : u64,
}

pub(crate) fn sys_openat2(args : SyscallArgs) -> UserRet {
    let dirfd = args.arg(0) as isize;
    let path_ptr = args.arg(1);
    let how_ptr = args.arg(2);
    let size = args.arg(3);
    let how = match read_open_how(how_ptr, size) {
        Ok(how) => how,
        Err(error) => return UserRet::from_error(error),
    };
    if how.flags > u32::MAX as u64 || how.mode > u32::MAX as u64 ||
       how.resolve & !VALID_RESOLVE != 0
    {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let flags = how.flags as u32;
    let mode = how.mode as u32;
    if flags & !VALID_OPEN_FLAGS != 0 || mode & !0o7777 != 0 ||
       mode != 0 && flags & O_CREAT == 0 && flags & O_TMPFILE != O_TMPFILE
    {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    if how.resolve & RESOLVE_CACHED != 0 {
        // WaterOS 尚无完整 dcache-only 路径；不能执行 I/O 后伪装缓存命中。
        return UserRet::from_error(ErrNo::EAGAIN);
    }
    if how.resolve & (RESOLVE_IN_ROOT | RESOLVE_BENEATH) ==
       (RESOLVE_IN_ROOT | RESOLVE_BENEATH)
    {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let user_path = match copy_user_path_cstr(path_ptr, crate::user_copy::USER_PATH_MAX) {
        Ok(path) => path,
        Err(error) => return UserRet::from_error(error),
    };
    if user_path.is_empty() {
        return UserRet::from_error(ErrNo::ENOENT);
    }
    if how.resolve & RESOLVE_IN_ROOT != 0 {
        return open_in_root(dirfd, user_path.as_str(), flags, mode, how.resolve);
    }
    if how.resolve & !(RESOLVE_NO_MAGICLINKS) == 0 {
        return openat_path(dirfd, user_path.as_str(), flags, mode);
    }
    if how.resolve & RESOLVE_BENEATH != 0 &&
       (user_path.starts_with('/') || has_parent_component(user_path.as_str()))
    {
        return UserRet::from_error(ErrNo::EXDEV);
    }
    let lexical = match resolve_path_at(dirfd, user_path.as_str()) {
        Ok(path) => path,
        Err(error) => return UserRet::from_error(error),
    };
    if how.resolve & RESOLVE_NO_SYMLINKS != 0 {
        if let Err(error) = reject_any_symlink(lexical.as_str()) {
            return UserRet::from_error(error);
        }
    }
    let final_path = match resolve_for_open(lexical.as_str(), flags) {
        Ok(path) => path,
        Err(error) => return UserRet::from_error(error),
    };
    let base = match resolve_path_at(dirfd, ".") {
        Ok(path) => path,
        Err(error) => return UserRet::from_error(error),
    };
    if how.resolve & RESOLVE_BENEATH != 0 && !path_is_at_or_beneath(final_path.as_str(), base.as_str()) {
        return UserRet::from_error(ErrNo::EXDEV);
    }
    if how.resolve & RESOLVE_NO_XDEV != 0 &&
       vfs::mount_statfs_magic(base.as_str()) != vfs::mount_statfs_magic(final_path.as_str())
    {
        return UserRet::from_error(ErrNo::EXDEV);
    }

    // 已经得到受约束的规范化绝对路径；从 AT_FDCWD 打开不会再受调用者 cwd 变化影响。
    open_resolved_path(final_path.as_str(), flags, mode)
}

fn open_in_root(dirfd : isize, path : &str, flags : u32, mode : u32, resolve : u64) -> UserRet {
    let root = match resolve_path_at(dirfd, ".") {
        Ok(path) => path,
        Err(error) => return UserRet::from_error(error),
    };
    let lexical = match vfs::cwd::resolve_with_virtual_root(root.as_str(), root.as_str(), path) {
        Ok(path) => path,
        Err(error) => return UserRet::from_error(vfs_error_to_errno(error)),
    };
    if resolve & RESOLVE_NO_SYMLINKS != 0 {
        if let Err(error) = reject_any_symlink_in_root(lexical.as_str(), root.as_str()) {
            return UserRet::from_error(error);
        }
    }
    let final_path = match resolve_for_open_in_root(lexical.as_str(), root.as_str(), flags) {
        Ok(path) => path,
        Err(error) => return UserRet::from_error(error),
    };
    if resolve & RESOLVE_NO_XDEV != 0 &&
       vfs::mount_statfs_magic(root.as_str()) != vfs::mount_statfs_magic(final_path.as_str())
    {
        return UserRet::from_error(ErrNo::EXDEV);
    }
    open_resolved_path(final_path.as_str(), flags, mode)
}

fn resolve_for_open(path : &str, flags : u32) -> Result<String, ErrNo> {
    let final_symlink = if flags & O_NOFOLLOW != 0 {
        vfs::api::FinalSymlink::NoFollow
    } else {
        vfs::api::FinalSymlink::Follow
    };
    match resolve_symlinks(path, final_symlink) {
        Ok(path) => Ok(path),
        Err(ErrNo::ENOENT) if flags & O_CREAT != 0 => {
            let (parent, name) = path.rsplit_once('/').unwrap_or(("/", path));
            let parent = if parent.is_empty() { "/" } else { parent };
            let parent = resolve_symlinks(parent, vfs::api::FinalSymlink::Follow)?;
            vfs::api::resolve_against_cwd(parent.as_str(), Some(name)).map_err(vfs_error_to_errno)
        }
        Err(error) => Err(error),
    }
}

fn resolve_for_open_in_root(path : &str, root : &str, flags : u32) -> Result<String, ErrNo> {
    let final_symlink = if flags & O_NOFOLLOW != 0 {
        vfs::api::FinalSymlink::NoFollow
    } else {
        vfs::api::FinalSymlink::Follow
    };
    match vfs::resolve_symlink_in_root_absolute(path, root, final_symlink) {
        Ok(path) => Ok(path),
        Err(VfsError::NotFound) if flags & O_CREAT != 0 => {
            let (parent, name) = path.rsplit_once('/').unwrap_or((root, path));
            let parent = if parent.is_empty() { root } else { parent };
            let parent = vfs::resolve_symlink_in_root_absolute(parent,
                                                                root,
                                                                vfs::api::FinalSymlink::Follow)
                .map_err(vfs_error_to_errno)?;
            vfs::cwd::resolve_with_virtual_root(root, parent.as_str(), name)
                .map_err(vfs_error_to_errno)
        }
        Err(error) => Err(vfs_error_to_errno(error)),
    }
}

fn read_open_how(ptr : usize, size : usize) -> Result<OpenHow, ErrNo> {
    if ptr == 0 {
        return Err(ErrNo::EFAULT);
    }
    if size < OPEN_HOW_SIZE_VER0 {
        return Err(ErrNo::EINVAL);
    }
    if size > OPEN_HOW_MAX_SIZE {
        return Err(ErrNo::E2BIG);
    }
    let mut raw = Vec::new();
    raw.try_reserve_exact(size).map_err(|_| ErrNo::ENOMEM)?;
    raw.resize(size, 0);
    if copy_from_user(&mut raw, ptr)? != size {
        return Err(ErrNo::EFAULT);
    }
    if raw[OPEN_HOW_SIZE_VER0..].iter().any(|byte| *byte != 0) {
        return Err(ErrNo::E2BIG);
    }
    Ok(OpenHow { flags : u64::from_ne_bytes(raw[0..8].try_into().unwrap()),
                 mode : u64::from_ne_bytes(raw[8..16].try_into().unwrap()),
                 resolve : u64::from_ne_bytes(raw[16..24].try_into().unwrap()) })
}

fn reject_any_symlink(path : &str) -> Result<(), ErrNo> {
    let root = vfs::cwd::current_root().map_err(vfs_error_to_errno)?;
    reject_any_symlink_in_root(path, root.as_str())
}

fn reject_any_symlink_in_root(path : &str, root : &str) -> Result<(), ErrNo> {
    let suffix = if root == "/" {
        path
    } else if path == root {
        "/"
    } else {
        path.strip_prefix(root)
            .filter(|suffix| suffix.starts_with('/'))
            .ok_or(ErrNo::EACCES)?
    };
    let mut current = String::from(root);
    let components : Vec<&str> = suffix.split('/').filter(|component| !component.is_empty()).collect();
    for (index, component) in components.iter().enumerate() {
        if current != "/" {
            current.push('/');
        }
        current.push_str(component);
        match vfs::read_symlink_absolute(current.as_str()) {
            Ok(_) => return Err(ErrNo::ELOOP),
            Err(VfsError::NotAFile) => {}
            Err(VfsError::NotFound) if index + 1 == components.len() => return Ok(()),
            Err(VfsError::NotFound) => return Err(ErrNo::ENOENT),
            Err(error) => return Err(vfs_error_to_errno(error)),
        }
    }
    Ok(())
}

fn has_parent_component(path : &str) -> bool {
    path.split('/').any(|component| component == "..")
}

fn path_is_at_or_beneath(path : &str, base : &str) -> bool {
    base == "/" || path == base || path.starts_with(base) &&
                                path.as_bytes().get(base.len()) == Some(&b'/')
}

#[cfg(feature = "self_test")]
pub(crate) fn self_test() {
    assert!(has_parent_component("a/../b"));
    assert!(!has_parent_component("a/.../b"));
    assert!(path_is_at_or_beneath("/tmp/a", "/tmp"));
    assert!(!path_is_at_or_beneath("/tmp2/a", "/tmp"));
    assert_eq!(core::mem::size_of::<OpenHow>(), OPEN_HOW_SIZE_VER0);
}

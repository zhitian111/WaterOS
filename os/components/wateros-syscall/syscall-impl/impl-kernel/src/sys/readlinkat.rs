//! `readlinkat(2)`：早期 BusyBox/glibc 兼容路径。

extern crate alloc;

use alloc::string::String;
use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use cred::api::ProcessCredentials;
use vfs::active_impl;
use vfs::api::{resolve_against_cwd, SingleRootReadView, VfsError, VfsNodeType};

use crate::sys::path_at::resolve_path_at;
use crate::user_copy::{copy_to_user, copy_user_path_cstr, USER_PATH_MAX};
use crate::vfs_util::vfs_error_to_errno;

const PROC_SELF_EXE: &str = "/proc/self/exe";
const PROC_THREAD_SELF_EXE: &str = "/proc/thread-self/exe";

pub(crate) fn sys_readlinkat(args: SyscallArgs) -> UserRet {
    let dirfd = args.arg(0) as isize;
    let path_ptr = args.arg(1);
    let buf_ptr = args.arg(2);
    let bufsiz = args.arg(3);

    if buf_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if bufsiz == 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let path = match copy_user_path_cstr(path_ptr, USER_PATH_MAX) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };

    if path.is_empty() {
        return readlinkat_empty_path(dirfd, buf_ptr, bufsiz);
    }

    if path == PROC_SELF_EXE || path == PROC_THREAD_SELF_EXE {
        return read_current_exe(buf_ptr, bufsiz);
    }

    let resolved = match resolve_path_at(dirfd, path.as_str()) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };
    let resolved = match resolve_readlink_prefix_symlinks(resolved.as_str()) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };
    let cred = cred::current_credentials();
    if let Err(e) = check_parent_search(resolved.as_str(), &cred) {
        return UserRet::from_error(e);
    }

    match vfs::active_impl::backend().metadata(resolved.as_str()) {
        Ok(meta) if meta.node_type == VfsNodeType::Symlink => {
            read_symlink_target(resolved.as_str(), buf_ptr, bufsiz)
        }
        Ok(_) => UserRet::from_error(ErrNo::EINVAL),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

fn resolve_readlink_prefix_symlinks(path: &str) -> Result<String, ErrNo> {
    let mut current = String::from(path);
    'follow: for _ in 0..40 {
        let parts: alloc::vec::Vec<String> = current
            .trim_start_matches('/')
            .split('/')
            .filter(|part| !part.is_empty())
            .map(String::from)
            .collect();
        if parts.len() <= 1 {
            return Ok(current);
        }

        let mut prefix = String::from("/");
        for index in 0..parts.len() - 1 {
            append_component(&mut prefix, parts[index].as_str());
            match active_impl::backend().metadata(prefix.as_str()) {
                Ok(meta) if meta.node_type == VfsNodeType::Directory => {}
                Ok(meta) if meta.node_type == VfsNodeType::Symlink => {
                    let target = vfs::read_symlink_absolute(prefix.as_str())
                        .map_err(vfs_error_to_errno)?;
                    let target = core::str::from_utf8(target.as_slice())
                        .map_err(|_| ErrNo::EINVAL)?;
                    let parent = prefix
                        .rsplit_once('/')
                        .map(|(parent, _)| if parent.is_empty() { "/" } else { parent })
                        .unwrap_or("/");
                    let mut next = resolve_against_cwd(parent, Some(target))
                        .map_err(vfs_error_to_errno)?;
                    for part in &parts[index + 1..] {
                        append_component(&mut next, part.as_str());
                    }
                    current = next;
                    continue 'follow;
                }
                Ok(_) => return Err(ErrNo::ENOTDIR),
                Err(VfsError::NotFound) => return Err(ErrNo::ENOENT),
                Err(e) => return Err(vfs_error_to_errno(e)),
            }
        }
        return Ok(current);
    }
    Err(ErrNo::ELOOP)
}

fn readlinkat_empty_path(dirfd: isize, buf_ptr: usize, bufsiz: usize) -> UserRet {
    if dirfd < 0 {
        return UserRet::from_error(ErrNo::EBADF);
    }

    let path = match vfs::fd::with_current_io(dirfd as usize, |handle| {
        let meta = handle.metadata()?;
        if meta.node_type != VfsNodeType::Symlink {
            return Err(VfsError::Unsupported);
        }
        handle
            .backing_path()
            .map(String::from)
            .ok_or(VfsError::Unsupported)
    }) {
        Ok(path) => path,
        Err(VfsError::Unsupported) => return UserRet::from_error(ErrNo::EINVAL),
        Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
    };

    read_symlink_target(path.as_str(), buf_ptr, bufsiz)
}

fn append_component(path: &mut String, component: &str) {
    if path != "/" {
        path.push('/');
    }
    path.push_str(component);
}

fn check_parent_search(path: &str, cred: &ProcessCredentials) -> Result<(), ErrNo> {
    if cred.effective_uid.0 == 0 {
        return Ok(());
    }

    let parts: alloc::vec::Vec<&str> = path
        .trim_start_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() <= 1 {
        return Ok(());
    }

    let mut current = String::from("/");
    for part in &parts[..parts.len() - 1] {
        if current != "/" {
            current.push('/');
        }
        current.push_str(part);
        match active_impl::backend().metadata(current.as_str()) {
            Ok(meta) if meta.node_type == VfsNodeType::Directory => {
                if meta.mode & 0o111 == 0 {
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

fn read_symlink_target(path: &str, buf_ptr: usize, bufsiz: usize) -> UserRet {
    let bytes = match vfs::read_symlink_absolute(path) {
        Ok(data) => data,
        Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
    };
    let write_len = core::cmp::min(bytes.len(), bufsiz);
    if write_len == 0 {
        return UserRet::from_success(0);
    }
    match copy_to_user(buf_ptr, &bytes[..write_len]) {
        Ok(n) if n == write_len => UserRet::from_success(write_len),
        Ok(_) => UserRet::from_error(ErrNo::EFAULT),
        Err(e) => UserRet::from_error(e),
    }
}

fn read_current_exe(buf_ptr: usize, bufsiz: usize) -> UserRet {
    let exe_path = match vfs::cwd::current_exe_path() {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(vfs_error_to_errno(e)),
    };
    let bytes = exe_path.as_bytes();
    let write_len = core::cmp::min(bytes.len(), bufsiz);
    if write_len == 0 {
        return UserRet::from_success(0);
    }

    match copy_to_user(buf_ptr, &bytes[..write_len]) {
        Ok(n) if n == write_len => {
            if bufsiz > write_len {
                let nul = [0u8];
                if copy_to_user(buf_ptr + write_len, &nul).is_err() {
                    return UserRet::from_error(ErrNo::EFAULT);
                }
            }
            UserRet::from_success(write_len)
        }
        Ok(n) => {
            log::trace!(
                "[readlinkat] /proc/self/exe partial copy n={n} expected={write_len} \
                 buf={buf_ptr:#x} path={exe_path:?}"
            );
            UserRet::from_error(ErrNo::EFAULT)
        }
        Err(e) => {
            log::trace!(
                "[readlinkat] /proc/self/exe copy failed errno={e:?} buf={buf_ptr:#x} \
                 bufsiz={bufsiz} path={exe_path:?} task={:?} aspace_ptr={:#x} \
                 task_satp={:#x} trap_satp={:#x}",
                task::current_task_id(),
                task::current_task_user_aspace_ptr(),
                task::current_task_user_address_space_token(),
                task::current_task_trap_return_address_space_token(),
            );
            UserRet::from_error(e)
        }
    }
}

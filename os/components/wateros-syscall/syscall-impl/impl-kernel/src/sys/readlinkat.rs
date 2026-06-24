//! `readlinkat(2)`：早期 BusyBox/glibc 兼容路径。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::api::{SingleRootReadView, VfsNodeType};

use crate::sys::path_at::resolve_path_at;
use crate::user_copy::{copy_to_user, copy_user_path_cstr};
use crate::vfs_util::vfs_error_to_errno;

const PATH_MAX: usize = 256;
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

    let path = match copy_user_path_cstr(path_ptr, PATH_MAX) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };

    if path == PROC_SELF_EXE || path == PROC_THREAD_SELF_EXE {
        return read_current_exe(buf_ptr, bufsiz);
    }

    let resolved = match resolve_path_at(dirfd, path.as_str()) {
        Ok(path) => path,
        Err(e) => return UserRet::from_error(e),
    };

    match vfs::active_impl::backend().metadata(resolved.as_str()) {
        Ok(meta) if meta.node_type == VfsNodeType::Symlink => {
            read_symlink_target(resolved.as_str(), buf_ptr, bufsiz)
        }
        Ok(_) => UserRet::from_error(ErrNo::EINVAL),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
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

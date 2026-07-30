//! `pipe2(2)`：创建 pipe fd 对；支持 `O_NONBLOCK` / `O_DIRECT` 状态位。

//! 本模块代码由AI完成
use alloc::boxed::Box;

use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use crate::vfs_util::vfs_error_to_errno;

pub(crate) const O_NONBLOCK: usize = 0o0004000;
const O_DIRECT: usize = 0o00040000;
const O_CLOEXEC: usize = 0o2000000;
const FD_CLOEXEC: usize = 1;

// 本方法代码由AI完成
pub(crate) fn sys_pipe2(args: SyscallArgs) -> UserRet {
    let pipefd_ptr = args.arg(0);
    let flags = args.arg(1);
    if pipefd_ptr == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if flags & !(O_NONBLOCK | O_DIRECT | O_CLOEXEC) != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let task_id = match vfs::fd::current_task_id() {
        Ok(task_id) => task_id,
        Err(err) => return UserRet::from_error(vfs_error_to_errno(err)),
    };
    let nonblocking = flags & O_NONBLOCK != 0;
    let direct = flags & O_DIRECT != 0;
    let (read_end, write_end) = vfs::pipe_handle_pair_with_flags(nonblocking, direct);
    let (read_fd, write_fd) =
        match vfs::fd::with_registry(|reg| -> vfs::VfsResult<(usize, usize)> {
            let read_fd = reg.alloc_fd_for_task(task_id, Box::new(read_end))?;
            match reg.alloc_fd_for_task(task_id, Box::new(write_end)) {
                Ok(write_fd) => Ok((read_fd, write_fd)),
                Err(err) => {
                    let _ = reg.close_fd_for_task(task_id, read_fd);
                    Err(err)
                }
            }
        }) {
            Ok(fds) => fds,
            Err(err) => return UserRet::from_error(vfs_error_to_errno(err)),
        };
    if flags & O_CLOEXEC != 0 {
        if let Err(err) = vfs::fd::set_fd_flags(read_fd, FD_CLOEXEC) {
            let _ = vfs::fd::close_fd(read_fd);
            let _ = vfs::fd::close_fd(write_fd);
            return UserRet::from_error(vfs_error_to_errno(err));
        }
        if let Err(err) = vfs::fd::set_fd_flags(write_fd, FD_CLOEXEC) {
            let _ = vfs::fd::close_fd(read_fd);
            let _ = vfs::fd::close_fd(write_fd);
            return UserRet::from_error(vfs_error_to_errno(err));
        }
    }
    let fds = [
        read_fd as i32,
        write_fd as i32,
    ];
    match crate::user_copy::copy_to_user(pipefd_ptr, unsafe {
        core::slice::from_raw_parts(
            fds.as_ptr() as *const u8,
            core::mem::size_of_val(&fds),
        )
    }) {
        Ok(n) if n == core::mem::size_of_val(&fds) => UserRet::from_success(0),
        _ => {
            let _ = vfs::fd::close_fd(read_fd);
            let _ = vfs::fd::close_fd(write_fd);
            UserRet::from_error(ErrNo::EFAULT)
        }
    }
}

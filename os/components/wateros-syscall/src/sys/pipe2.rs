//! `pipe2(2)`：创建 pipe fd 对；支持 `O_NONBLOCK`。

use alloc::boxed::Box;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::{PipeReadHandle, PipeWriteHandle};

use crate::vfs_util::vfs_error_to_errno;

pub(crate) const O_NONBLOCK: usize = 0o0004000;

pub(crate) fn sys_pipe2(args: SyscallArgs) -> UserRet {
    let pipefd_ptr = args.arg(0) as *mut i32;
    let flags = args.arg(1);
    if pipefd_ptr.is_null() {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    if flags & !O_NONBLOCK != 0 {
        return UserRet::from_error(ErrNo::EINVAL);
    }
    let task_id = match vfs::fd::current_task_id() {
        Ok(task_id) => task_id,
        Err(err) => return UserRet::from_error(vfs_error_to_errno(err)),
    };
    let nonblocking = flags & O_NONBLOCK != 0;
    let (read_end, write_end) = ipc::pipe::PipeEndpoint::pair(nonblocking);
    let mut reg = vfs::fd::registry().exclusive_access();
    let read_fd = reg.alloc_fd_for_task(task_id, Box::new(PipeReadHandle(read_end)));
    let write_fd = reg.alloc_fd_for_task(task_id, Box::new(PipeWriteHandle(write_end)));
    drop(reg);
    unsafe {
        pipefd_ptr.write(read_fd as i32);
        pipefd_ptr.add(1).write(write_fd as i32);
    }
    UserRet::from_success(0)
}

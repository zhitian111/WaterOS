//! `pipe2(2)`：创建 pipe fd 对；支持 `O_NONBLOCK`。

use alloc::boxed::Box;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::{PipeReadHandle, PipeWriteHandle};

use crate::vfs_util::vfs_error_to_errno;

pub(crate) const O_NONBLOCK : usize = 0o0004000;

pub(crate) fn sys_pipe2(args : SyscallArgs) -> UserRet {
    let pipefd_ptr = args.arg(0);
    let flags = args.arg(1);
    if pipefd_ptr == 0 {
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
    let read_fd = reg.alloc_fd_for_task(task_id,
                                        Box::new(PipeReadHandle(read_end)));
    let write_fd = reg.alloc_fd_for_task(task_id,
                                         Box::new(PipeWriteHandle(write_end)));
    drop(reg);
    let fds = [read_fd as i32, write_fd as i32];
    match crate::user_copy::copy_to_user(pipefd_ptr, unsafe {
              core::slice::from_raw_parts(fds.as_ptr() as *const u8,
                                          core::mem::size_of_val(&fds))
          }) {
        Ok(n) if n == core::mem::size_of_val(&fds) => UserRet::from_success(0),
        _ => UserRet::from_error(ErrNo::EFAULT),
    }
}

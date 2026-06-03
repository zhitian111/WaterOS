//! `poll(2)`（号 271）：委托共享 [`poll_engine`]。

use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::poll_engine::{PollDeadline, do_poll_with_deadline};

pub(crate) fn sys_poll(args: SyscallArgs) -> UserRet {
    let fds_ptr = args.arg(0);
    let nfds = args.arg(1);
    let timeout_ms = args.arg(2) as isize;
    let deadline = match PollDeadline::from_poll_millis(timeout_ms) {
        Ok(d) => d,
        Err(e) => return UserRet::from_error(e),
    };
    do_poll_with_deadline(fds_ptr, nfds, deadline)
}

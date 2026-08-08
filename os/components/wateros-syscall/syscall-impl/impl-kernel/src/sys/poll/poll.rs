//! `poll(2)` 兼容入口：generic ABI 无独立 nr，用户态通常经 `ppoll`(73) 进入。

//! 本模块代码由AI完成
use api_v0::SyscallArgs;
use api_v0::UserRet;

use crate::poll_engine::{PollDeadline, do_poll_with_deadline};

// 本方法代码由AI完成
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

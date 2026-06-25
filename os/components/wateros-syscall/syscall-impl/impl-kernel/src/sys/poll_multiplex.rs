//! `ppoll`(73) / `pselect6`(72) / `select`(23)。

use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::poll_engine::{
    install_poll_sigmask, PollDeadline, do_poll_with_deadline, do_pselect_with_deadline,
};

pub(crate) fn sys_ppoll(args: SyscallArgs) -> UserRet {
    let fds_ptr = args.arg(0);
    let nfds = args.arg(1);
    let timeout_ptr = args.arg(2);
    let sigmask_ptr = args.arg(3);
    let sigsetsize = args.arg(4);

    let _sigmask_guard = match install_poll_sigmask(sigmask_ptr, sigsetsize) {
        Ok(guard) => guard,
        Err(e) => return UserRet::from_error(e),
    };

    let deadline = match PollDeadline::from_timespec_ptr(timeout_ptr) {
        Ok(d) => d,
        Err(e) => return UserRet::from_error(e),
    };
    do_poll_with_deadline(fds_ptr, nfds, deadline)
}

pub(crate) fn sys_pselect6(args: SyscallArgs) -> UserRet {
    let nfds = args.arg(0);
    let readfds = args.arg(1);
    let writefds = args.arg(2);
    let exceptfds = args.arg(3);
    let timeout_ptr = args.arg(4);
    let sigmask_ptr = args.arg(5);
    const RT_SIGSET_SIZE: usize = 8;

    let _sigmask_guard = match install_poll_sigmask(sigmask_ptr, RT_SIGSET_SIZE) {
        Ok(guard) => guard,
        Err(e) => return UserRet::from_error(e),
    };

    let deadline = match PollDeadline::from_timespec_ptr(timeout_ptr) {
        Ok(d) => d,
        Err(e) => return UserRet::from_error(e),
    };
    do_pselect_with_deadline(nfds, readfds, writefds, exceptfds, deadline)
}

pub(crate) fn sys_select(args: SyscallArgs) -> UserRet {
    let nfds = args.arg(0);
    let readfds = args.arg(1);
    let writefds = args.arg(2);
    let exceptfds = args.arg(3);
    let timeout_ptr = args.arg(4);

    let deadline = match PollDeadline::from_timeval_ptr(timeout_ptr) {
        Ok(d) => d,
        Err(e) => return UserRet::from_error(e),
    };
    do_pselect_with_deadline(nfds, readfds, writefds, exceptfds, deadline)
}

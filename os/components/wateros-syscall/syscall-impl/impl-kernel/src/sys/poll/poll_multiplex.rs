//! `ppoll`(73) / `pselect6`(72) / `select`(23)。

//! 本模块代码由AI完成
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use abi::errno::ErrNo;

use crate::poll_engine::{
    install_poll_sigmask, PollDeadline, do_poll_with_deadline, do_pselect_with_deadline,
};
use crate::user_copy::copy_from_user_struct;

#[repr(C)]
#[derive(Clone, Copy)]
struct Pselect6Sigmask {
    sigmask : usize,
    sigsetsize : usize,
}

// 本方法代码由AI完成
pub(crate) fn sys_ppoll(args: SyscallArgs) -> UserRet {
    let fds_ptr = args.arg(0);
    let nfds = args.arg(1);
    let timeout_ptr = args.arg(2);
    let sigmask_ptr = args.arg(3);
    let sigsetsize = args.arg(4);

    let sigmask_guard = match install_poll_sigmask(sigmask_ptr, sigsetsize) {
        Ok(guard) => guard,
        Err(e) => return UserRet::from_error(e),
    };

    let deadline = match PollDeadline::from_timespec_ptr(timeout_ptr) {
        Ok(d) => d,
        Err(e) => return UserRet::from_error(e),
    };
    let result = do_poll_with_deadline(fds_ptr, nfds, deadline);
    if let Some(guard) = sigmask_guard {
        guard.finish(result.0 == ErrNo::EINTR.user_ret());
    }
    result
}

// 本方法代码由AI完成
pub(crate) fn sys_pselect6(args: SyscallArgs) -> UserRet {
    let nfds = args.arg(0);
    let readfds = args.arg(1);
    let writefds = args.arg(2);
    let exceptfds = args.arg(3);
    let timeout_ptr = args.arg(4);
    let sigmask_arg = args.arg(5);
    let (sigmask_ptr, sigsetsize) = if sigmask_arg == 0 {
        (0, 0)
    } else {
        match copy_from_user_struct::<Pselect6Sigmask>(sigmask_arg) {
            Ok(argument) => (argument.sigmask, argument.sigsetsize),
            Err(error) => return UserRet::from_error(error),
        }
    };

    let sigmask_guard = match install_poll_sigmask(sigmask_ptr, sigsetsize) {
        Ok(guard) => guard,
        Err(e) => return UserRet::from_error(e),
    };

    let deadline = match PollDeadline::from_timespec_ptr(timeout_ptr) {
        Ok(d) => d,
        Err(e) => return UserRet::from_error(e),
    };
    let result = do_pselect_with_deadline(nfds, readfds, writefds, exceptfds, deadline);
    if let Some(guard) = sigmask_guard {
        guard.finish(result.0 == ErrNo::EINTR.user_ret());
    }
    result
}

// 本方法代码由AI完成
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

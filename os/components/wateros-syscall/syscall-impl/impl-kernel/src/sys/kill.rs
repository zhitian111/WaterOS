//! `kill(2)` — 向任务发送信号；首期实现终止类信号的强制退出语义。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use ipc::signal::SignalDelivery;
use task::TaskExitCode;

/// Linux 标准信号号上界（不含实时信号）。
const _NSIG: i32 = 64;

/// 与 wait 状态字节一致：`(sig & 0x7f) << 8`。
fn exit_code_for_signal(sig: i32) -> TaskExitCode {
    ((sig & 0x7f) as isize) << 8
}

fn resolve_task_id(pid: isize) -> Result<usize, ErrNo> {
    if pid <= 0 {
        return Err(ErrNo::EINVAL);
    }
    let task_id = pid as usize;
    if task::task_snapshot(task_id).is_none() {
        return Err(ErrNo::ESRCH);
    }
    Ok(task_id)
}

/// `kill(pid, sig)` — riscv64 系统调用号 129。
pub(crate) fn sys_kill(args: SyscallArgs) -> UserRet {
    let pid = args.arg(0) as isize;
    let sig = args.arg(1) as i32;

    if sig < 0 || sig >= _NSIG {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let task_id = match resolve_task_id(pid) {
        Ok(id) => id,
        Err(e) => return UserRet::from_error(e),
    };

    if sig == 0 {
        return UserRet::from_success(0);
    }

    let delivery = match ipc::signal::with_registry(|registry| registry.send(task_id, sig as usize)) {
        Ok(delivery) => delivery,
        Err(_) => return UserRet::from_error(ErrNo::EINVAL),
    };

    if !matches!(delivery, SignalDelivery::Terminate) {
        return UserRet::from_success(0);
    }

    let exit_code = exit_code_for_signal(sig);
    let current = task::current_task_id();

    if current == Some(task_id) {
        super::robust::robust_exit_cleanup(task_id);
        task::exit_current(exit_code);
    }

    super::robust::robust_exit_cleanup(task_id);
    if task::kill_task(task_id, exit_code) {
        UserRet::from_success(0)
    } else {
        UserRet::from_error(ErrNo::ESRCH)
    }
}

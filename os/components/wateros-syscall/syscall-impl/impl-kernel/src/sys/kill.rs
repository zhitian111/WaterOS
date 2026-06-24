//! `kill(2)` — 向任务发送信号；首期实现终止类信号的强制退出语义。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use ipc::signal::SignalDelivery;
use task::{ProcessId, TaskId};

/// Linux 标准信号号上界（不含实时信号）。
const _NSIG : i32 = 64;

fn resolve_task_id(pid : isize) -> Result<TaskId, ErrNo> {
    if pid <= 0 {
        return Err(ErrNo::EINVAL);
    }
    task::leader_task_for_process(ProcessId::from_raw(pid as usize)).ok_or(ErrNo::ESRCH)
}

/// `kill(pid, sig)` — riscv64 系统调用号 129。
pub(crate) fn sys_kill(args : SyscallArgs) -> UserRet {
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

    let process = match task::process_task_snapshot(task_id) {
        Some(snapshot) => snapshot.pid,
        None => return UserRet::from_error(ErrNo::ESRCH),
    };
    if super::signal::ensure_process_signal_state(process).is_err() {
        return UserRet::from_error(ErrNo::ESRCH);
    }
    let dispatch = match ipc::signal::with_registry(|registry| {
              registry.send_process(process.raw(), sig as usize)
          }) {
        Ok(dispatch) => dispatch,
        Err(_) => return UserRet::from_error(ErrNo::EINVAL),
    };

    super::signal::apply_signal_dispatch(dispatch, sig as usize);
    if dispatch.delivery == SignalDelivery::Pending {
        if let Some(task_ids) = task::task_ids_for_process(process) {
            for member in task_ids {
                let deliverable = ipc::signal::with_registry(|registry| {
                    registry.has_deliverable(member).unwrap_or(false)
                });
                if deliverable {
                    let _ = task::interrupt_task(member);
                }
            }
        }
    }
    UserRet::from_success(0)
}

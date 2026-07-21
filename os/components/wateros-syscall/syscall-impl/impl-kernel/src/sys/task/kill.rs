//! `kill(2)` — 向任务发送信号；首期实现终止类信号的强制退出语义。

//! 本模块代码由AI完成
extern crate alloc;

use alloc::vec::Vec;

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use ipc::signal::SignalDelivery;
use task::ProcessId;

/// Linux 标准信号号上界（不含实时信号）。
const _NSIG: i32 = 64;

fn collect_process_tree(root: ProcessId) -> Vec<ProcessId> {
    let mut targets = Vec::new();
    if task::process_snapshot(root).is_none() {
        return targets;
    }

    targets.push(root);
    let mut scan = 0;
    while scan < targets.len() {
        let parent = targets[scan];
        scan += 1;
        for pid in task::all_process_pids() {
            if targets.contains(&pid) {
                continue;
            }
            let Some(snapshot) = task::process_snapshot(pid) else {
                continue;
            };
            if snapshot.parent_pid == Some(parent) {
                targets.push(pid);
            }
        }
    }
    targets
}

fn resolve_kill_targets(pid: isize) -> Result<Vec<ProcessId>, ErrNo> {
    match pid {
        p if p > 0 => Ok(alloc::vec![ProcessId::from_raw(p as usize)]),
        0 => {
            let current = task::current_process_snapshot().ok_or(ErrNo::ESRCH)?;
            Ok(alloc::vec![current.pid])
        }
        -1 => {
            let current = task::current_process_snapshot().ok_or(ErrNo::ESRCH)?;
            Ok(task::all_process_pids()
                .into_iter()
                .filter(|p| *p != current.pid && p.raw() != 1)
                .collect())
        }
        _ => {
            let pgid = pid.unsigned_abs();
            let targets = collect_process_tree(ProcessId::from_raw(pgid));
            if targets.is_empty() {
                Err(ErrNo::ESRCH)
            } else {
                Ok(targets)
            }
        }
    }
}

fn send_signal_to_process(process: ProcessId, sig: usize) -> Result<(), ErrNo> {
    if task::leader_task_for_process(process).is_none() {
        return Err(ErrNo::ESRCH);
    }
    if super::signal::ensure_process_signal_state(process).is_err() {
        return Err(ErrNo::ESRCH);
    }
    let dispatch = ipc::signal::with_registry(|registry| registry.send_process(process.raw(), sig))
        .map_err(|_| ErrNo::EINVAL)?;
    super::signal::apply_signal_dispatch(dispatch, sig);
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
    Ok(())
}

/// `kill(pid, sig)` — riscv64 系统调用号 129。
// 本方法代码由AI完成
pub(crate) fn sys_kill(args: SyscallArgs) -> UserRet {
    let pid = args.arg(0) as isize;
    let sig = args.arg(1) as i32;

    if sig < 0 || sig >= _NSIG {
        return UserRet::from_error(ErrNo::EINVAL);
    }

    let targets = match resolve_kill_targets(pid) {
        Ok(targets) => targets,
        Err(e) => return UserRet::from_error(e),
    };
    if targets.is_empty() {
        return UserRet::from_error(ErrNo::ESRCH);
    }

    if sig == 0 {
        for process in &targets {
            if task::leader_task_for_process(*process).is_none() {
                return UserRet::from_error(ErrNo::ESRCH);
            }
        }
        return UserRet::from_success(0);
    }

    let mut sent = false;
    let mut last_err = ErrNo::ESRCH;
    for process in targets {
        match send_signal_to_process(process, sig as usize) {
            Ok(()) => sent = true,
            Err(e) => last_err = e,
        }
    }
    if sent {
        UserRet::from_success(0)
    } else {
        UserRet::from_error(last_err)
    }
}

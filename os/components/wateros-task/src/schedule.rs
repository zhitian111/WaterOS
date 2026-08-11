//! 调度、等待、唤醒与当前任务查询接口。

use crate::{
    active_impl, scheduler, ExitedTask, ProcessState, TaskId, TaskSnapshot, TaskState, TaskTick,
    TaskWaitResult, TaskWaitTarget,
};

/// 让当前任务主动让出 CPU。
pub fn yield_now() { scheduler::suspend_current_and_run_next(); }

/// 通知任务系统发生了一次时钟 tick。
pub fn schedule_tick() { scheduler::schedule_tick(); }

/// Respond to a remote reschedule request without changing scheduler time.
pub fn schedule_reschedule() { scheduler::schedule_reschedule(); }

/// 以指定阻塞原因挂起当前任务。
pub fn block_current(reason : TaskWaitTarget) { scheduler::block_current(reason); }

/// 让当前任务等待指定的阻塞对象。
pub fn wait_on(target : TaskWaitTarget) -> TaskWaitResult { scheduler::wait_current(target) }

/// 在调度临界区内复查条件；条件仍成立才等待指定的阻塞对象。
pub fn wait_on_while(target : TaskWaitTarget, condition : impl FnOnce() -> bool) -> TaskWaitResult {
    scheduler::wait_current_while(target, condition)
}

/// 让当前任务等待指定的阻塞对象，并带一个超时。
pub fn wait_on_for_ticks(target : TaskWaitTarget, timeout_ticks : TaskTick) -> TaskWaitResult {
    scheduler::wait_current_timeout(target, timeout_ticks)
}

/// 在调度临界区内复查条件；条件仍成立才带超时等待指定阻塞对象。
pub fn wait_on_while_for_ticks(target : TaskWaitTarget,
                               timeout_ticks : TaskTick,
                               condition : impl FnOnce() -> bool)
                               -> TaskWaitResult {
    scheduler::wait_current_timeout_while(target, timeout_ticks, condition)
}

/// 让当前任务等待指定任务退出。
pub fn wait_for_task_exit(task_id : TaskId) -> TaskWaitResult {
    wait_on(TaskWaitTarget::TaskExit(task_id))
}

/// 返回“等待指定任务退出”的通用等待句柄。

/// 让当前任务等待指定任务退出，并带一个超时。
pub fn wait_for_task_exit_for_ticks(task_id : TaskId, timeout_ticks : TaskTick) -> TaskWaitResult {
    wait_on_for_ticks(TaskWaitTarget::TaskExit(task_id),
                      timeout_ticks)
}

/// 让当前任务睡眠指定数量的 tick。
pub fn sleep_for_ticks(ticks : TaskTick) -> TaskWaitResult {
    scheduler::sleep_current_for_ticks(ticks)
}

/// 尝试唤醒指定任务。
pub fn wake_task(task_id : TaskId) -> bool { scheduler::wake_task(task_id) }

/// 以 `Interrupted` 结果将指定任务从等待与超时队列中同时移除。
pub fn interrupt_task(task_id : TaskId) -> bool { scheduler::interrupt_task(task_id) }

/// 回收指定已退出任务的信息。
pub fn reap_exited_task(task_id : TaskId) -> Option<ExitedTask> {
    let leader_pid = active_impl::process_task_snapshot(task_id).and_then(|process_task| {
                         let process = active_impl::process_snapshot(process_task.pid)?;
                         if process.leader_task_id != task_id {
                             return None;
                         }
                         if !matches!(process.state, ProcessState::Exited(_)) {
                             return None;
                         }
                         Some(process_task.pid)
                     });
    let exited = scheduler::reap_exited_task(task_id)?;
    if let Some(pid) = leader_pid {
        let _ = active_impl::reap_process(pid);
    }
    Some(exited)
}

/// 返回当前正在运行任务的任务号。
pub fn current_task_id() -> Option<TaskId> { scheduler::current_task_id() }

/// 返回当前正在运行任务的稳定快照。
pub fn current_task_snapshot() -> Option<TaskSnapshot> { scheduler::current_task_snapshot() }

/// 返回指定任务的稳定快照；任务不存在或已被回收时返回 `None`。
pub fn task_snapshot(task_id : TaskId) -> Option<TaskSnapshot> { scheduler::task_snapshot(task_id) }

/// 返回指定任务的调度生命周期状态；任务不存在时返回 `None`。
pub fn task_state(task_id : TaskId) -> Option<TaskState> { scheduler::task_state(task_id) }

/// 输出全部非 idle 任务的状态，用于定位长时间无用户态进展时的等待链。
pub fn log_stall_diagnostics() {
    let snapshots = scheduler::diagnostic_task_snapshots();
    log::warn!("[stall-debug][tasks] tick={} active_non_idle={}",
               current_tick(),
               snapshots.len());
    for snapshot in snapshots {
        let wait_queue_name = match snapshot.state {
            crate::TaskState::Blocking(TaskWaitTarget::WaitQueue(wait_queue_id)) => {
                scheduler::wait_queue_name(wait_queue_id)
            }
            _ => None,
        };
        log::warn!("[stall-debug][task] id={} parent={:?} kind={:?} state={:?} wait={:?} \
                    policy={:?} ready_cpu={:?} running_cpu={:?} last_cpu={:?} schedules={} \
                    ticks={} aspace={:#x}",
                   snapshot.id,
                   snapshot.parent_id,
                   snapshot.kind,
                   snapshot.state,
                   wait_queue_name,
                   snapshot.policy,
                   snapshot.ready_cpu_id,
                   snapshot.running_cpu_id,
                   snapshot.last_cpu_id,
                   snapshot.stats
                           .schedule_count,
                   snapshot.stats
                           .tick_count,
                   snapshot.user_aspace_ptr);
    }
}

/// 返回当前调度器逻辑 tick。
pub fn current_tick() -> TaskTick { scheduler::current_tick() }

/// 当前运行任务的用户地址空间指针（内核任务为 0）；基于 `current_task_snapshot` 的单字段便捷封装。
pub fn current_task_user_aspace_ptr() -> usize {
    scheduler::current_task_user_aspace_ptr()
}

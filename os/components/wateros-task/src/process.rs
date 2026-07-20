//! 进程语义、资源限制、wait 事件与进程清理接口。

use alloc::vec::Vec;

use crate::{
    active_impl, scheduler, ExitedTask, ProcessDescriptor, ProcessId, ProcessState,
    ProcessTaskDescriptor, ResourceLimit, SetResourceLimitError, TaskClearTid, TaskId, TaskState,
    TaskWaitTarget, ThreadId,
};

/// 查询进程语义快照；第一阶段仅供内部 bring-up / 后续 syscall 迁移使用。
pub fn process_snapshot(pid : ProcessId) -> Option<ProcessDescriptor> {
    active_impl::lookup_process(pid)
}

/// 返回 registry 中全部进程 PID（含未 reap 的 zombie）。
pub fn all_process_pids() -> Vec<ProcessId> { active_impl::all_process_pids() }

/// 返回进程内全部调度实体 `TaskId`（供 syscall robust 清理等路径使用）。
pub fn task_ids_for_process(pid : ProcessId) -> Option<Vec<TaskId>> {
    active_impl::task_ids_for_process(pid)
}

/// 回收指定进程中已经退出的非 leader 线程。
pub fn reap_exited_member_threads(pid : ProcessId) -> Vec<ExitedTask> {
    scheduler::reap_exited_tasks_atomic(|| {
        active_impl::take_exited_member_tasks(pid).unwrap_or_default()
    })
}

/// 按用户态线程号反查内部调度实体。
pub fn task_id_for_thread(tid : ThreadId) -> Option<TaskId> { active_impl::task_id_for_thread(tid) }

/// 判断指定 task 退出后，其所属进程是否没有其它仍运行的 task。
pub fn task_exit_would_finish_process(task_id : TaskId) -> Option<bool> {
    active_impl::task_exit_would_finish_process(task_id)
}

/// 查询进程内任务语义快照。
pub fn process_task_snapshot(task_id : TaskId) -> Option<ProcessTaskDescriptor> {
    active_impl::lookup_task(task_id)
}

/// 按调度实体 `TaskId` 反查其进程归属快照。
pub fn process_task_snapshot_by_task(task_id : TaskId) -> Option<ProcessTaskDescriptor> {
    active_impl::lookup_task(task_id)
}

/// 当前运行任务对应的进程归属快照；未接入真实 spawn 前可能为 `None`。
pub fn current_process_task_snapshot() -> Option<ProcessTaskDescriptor> {
    let task_id = crate::schedule::current_task_id()?;
    process_task_snapshot_by_task(task_id)
}

/// 当前运行任务的用户态线程 ID。
pub fn current_thread_id() -> Option<ThreadId> {
    current_process_task_snapshot().map(|snapshot| snapshot.tid)
}

/// 当前运行任务所属进程快照。
pub fn current_process_snapshot() -> Option<ProcessDescriptor> {
    let pid = current_process_task_snapshot()?.pid;
    process_snapshot(pid)
}

/// 查询进程已设置的资源限制；未设置时返回 `None`（由 syscall 层回退默认值）。
pub fn process_resource_limit(pid : ProcessId, resource : usize) -> Option<ResourceLimit> {
    active_impl::get_process_rlimit(pid, resource)
}

/// 为进程写入资源限制。
pub fn set_process_resource_limit(pid : ProcessId,
                                  resource : usize,
                                  limit : ResourceLimit)
                                  -> Result<(), SetResourceLimitError> {
    active_impl::set_process_rlimit(pid, resource, limit)
}

/// 查询进程 nice 值。
pub fn process_nice(pid : ProcessId) -> Option<i32> { active_impl::get_process_nice(pid) }

/// 设置进程 nice 值。调整进程 CPU 调度优先级
pub fn set_process_nice(pid : ProcessId, nice : i32) -> bool {
    active_impl::set_process_nice(pid, nice)
}

/// 查询进程组 id。
pub fn process_pgid(pid : ProcessId) -> Option<ProcessId> { active_impl::get_process_pgid(pid) }

/// 设置进程组 id。
pub fn set_process_pgid(pid : ProcessId, pgid : ProcessId) -> bool {
    active_impl::set_process_pgid(pid, pgid)
}

/// 将 nice 写入同一 pgid 下全部进程。
pub fn set_nice_for_pgid(pgid : ProcessId, nice : i32) -> bool {
    active_impl::set_nice_for_pgid(pgid, nice)
}

/// 进程组内最小 nice。
pub fn min_nice_in_pgid(pgid : ProcessId) -> Option<i32> { active_impl::min_nice_in_pgid(pgid) }

/// 进程是否仍占 registry 槽位。
pub fn process_exists(pid : ProcessId) -> bool { active_impl::process_exists(pid) }

/// 进程组是否仍有成员。
pub fn pgid_has_members(pgid : ProcessId) -> bool { active_impl::pgid_has_members(pgid) }

/// 按调度实体查询 `RLIMIT_NOFILE` 软限制；无进程上下文时回退 1024。
pub fn nofile_rlimit_for_task(task_id : TaskId) -> u64 {
    const RLIMIT_NOFILE : usize = 7;
    const DEFAULT_NOFILE : u64 = 1024;
    let pid = match process_task_snapshot(task_id) {
        Some(snapshot) => snapshot.pid,
        None => return DEFAULT_NOFILE,
    };
    process_resource_limit(pid, RLIMIT_NOFILE).map(|limit| limit.cur)
                                              .unwrap_or(DEFAULT_NOFILE)
}

/// 按进程号查找 leader task。
pub fn leader_task_for_process(pid : ProcessId) -> Option<TaskId> {
    active_impl::leader_task_for_process(pid)
}

/// 查找当前进程下一个已退出子进程。
pub fn find_exited_child_process(parent_pid : ProcessId) -> Option<ProcessDescriptor> {
    active_impl::find_exited_child_process(parent_pid)
}

/// 查找当前进程在指定进程组内下一个已退出子进程。
pub fn find_exited_child_process_in_pgid(parent_pid : ProcessId,
                                         pgid : ProcessId)
                                         -> Option<ProcessDescriptor> {
    active_impl::find_exited_child_process_in_pgid(parent_pid, pgid)
}

/// 查找父进程下一个 stopped 子进程。
pub fn find_stopped_child_process(parent_pid : ProcessId) -> Option<ProcessDescriptor> {
    active_impl::find_stopped_child_process(parent_pid)
}

/// 指定 stopped 子进程是否可被 wait。
pub fn stopped_child_ready_for_wait(parent_pid : ProcessId,
                                    child_pid : ProcessId)
                                    -> Option<ProcessDescriptor> {
    active_impl::stopped_child_ready_for_wait(parent_pid, child_pid)
}

/// 在 pgid 内查找 stopped 子进程。
pub fn find_stopped_child_process_in_pgid(parent_pid : ProcessId,
                                          pgid : ProcessId)
                                          -> Option<ProcessDescriptor> {
    active_impl::find_stopped_child_process_in_pgid(parent_pid, pgid)
}

/// 查找父进程下一个 continued 子进程。
pub fn find_continued_child_process(parent_pid : ProcessId) -> Option<ProcessDescriptor> {
    active_impl::find_continued_child_process(parent_pid)
}

/// 指定 continued 子进程是否可被 wait。
pub fn continued_child_ready_for_wait(parent_pid : ProcessId,
                                      child_pid : ProcessId)
                                      -> Option<ProcessDescriptor> {
    active_impl::continued_child_ready_for_wait(parent_pid, child_pid)
}

/// 在 pgid 内查找 continued 子进程。
pub fn find_continued_child_process_in_pgid(parent_pid : ProcessId,
                                            pgid : ProcessId)
                                            -> Option<ProcessDescriptor> {
    active_impl::find_continued_child_process_in_pgid(parent_pid, pgid)
}

/// 将进程标为 SIGSTOP 停止态。
pub fn mark_process_stopped(pid : ProcessId, signo : u8) -> bool {
    active_impl::mark_process_stopped(pid, signo)
}

/// 将进程从 stopped 恢复为 running。
pub fn mark_process_continued(pid : ProcessId) -> bool { active_impl::mark_process_continued(pid) }

/// 消费 stop 事件的 wait 可见性。
pub fn consume_stop_wait(pid : ProcessId, nowait : bool) {
    active_impl::consume_stop_wait(pid, nowait)
}

/// 消费 continued 事件的 wait 可见性。
pub fn consume_continued_wait(pid : ProcessId, nowait : bool) {
    active_impl::consume_continued_wait(pid, nowait)
}

/// 阻塞进程内所有尚未退出的任务（SIGSTOP）。
pub fn stop_process_tasks(pid : ProcessId) {
    let Some(task_ids) = task_ids_for_process(pid) else {
        return;
    };
    for task_id in task_ids {
        if !scheduler::task_snapshot(task_id).is_some_and(|snapshot| {
                                                 matches!(snapshot.state, TaskState::Exited(_))
                                             })
        {
            if crate::schedule::current_task_id() == Some(task_id) {
                crate::schedule::block_current(TaskWaitTarget::Manual);
            } else {
                scheduler::block_task_manual(task_id);
            }
        }
    }
}

/// 恢复进程内被 SIGSTOP 挂起的任务（SIGCONT）。
pub fn continue_process_tasks(pid : ProcessId) {
    let Some(task_ids) = task_ids_for_process(pid) else {
        return;
    };
    for task_id in task_ids {
        let _ = scheduler::wake_task(task_id);
    }
}

/// 子进程状态变化时唤醒正在 wait 的父进程。
pub fn wake_parent_child_waiters(child_pid : ProcessId) {
    let Some(child) = process_snapshot(child_pid) else {
        return;
    };
    let Some(parent_pid) = child.parent_pid else {
        return;
    };
    if let Some(leader) = leader_task_for_process(parent_pid) {
        scheduler::wake_child_exit_waiters(leader);
    }
}

/// 判断当前进程在指定进程组内是否仍有子进程。
pub fn has_child_process_in_pgid(parent_pid : ProcessId, pgid : ProcessId) -> bool {
    active_impl::has_child_process_in_pgid(parent_pid, pgid)
}

/// 为进程创建新会话。
pub fn create_session_for_process(pid : ProcessId) -> Result<(), ()> {
    active_impl::create_session_for_process(pid)
}

/// 查询进程 dumpable 标志。
pub fn process_dumpable(pid : ProcessId) -> Option<bool> { active_impl::process_dumpable(pid) }

/// 设置进程 dumpable 标志。
pub fn set_process_dumpable(pid : ProcessId, dumpable : bool) -> bool {
    active_impl::set_process_dumpable(pid, dumpable)
}

/// 查询 child subreaper 标志。
pub fn process_child_subreaper(pid : ProcessId) -> Option<bool> {
    active_impl::process_child_subreaper(pid)
}

/// 设置 child subreaper 标志。
pub fn set_process_child_subreaper(pid : ProcessId, enabled : bool) -> bool {
    active_impl::set_process_child_subreaper(pid, enabled)
}

/// 判断当前进程是否仍有子进程。
pub fn has_child_process(parent_pid : ProcessId) -> bool {
    active_impl::has_child_process(parent_pid)
}

/// bring-up 脚本结束后清理统计。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ProcessPurgeStats {
    /// 被强制 kill 的用户任务数。
    pub killed_tasks : usize,
    /// 被 reap 的已退出进程数。
    pub reaped_processes : usize,
}

/// 强制结束并回收 registry 中全部用户进程（含 basic 测试遗留的 Running 孤儿）。
pub fn purge_all_user_processes() -> (ProcessPurgeStats, Vec<ExitedTask>) {
    let mut stats = ProcessPurgeStats::default();
    let mut reaped_tasks = Vec::new();
    for _ in 0..256 {
        let pids = active_impl::all_process_pids();
        if pids.is_empty() {
            break;
        }
        let mut progress = false;
        for pid in &pids {
            let Some(snapshot) = active_impl::lookup_process(*pid) else {
                continue;
            };
            if matches!(snapshot.state, ProcessState::Exited(_)) {
                continue;
            }
            let Some(task_ids) = active_impl::task_ids_for_process(*pid) else {
                continue;
            };
            for task_id in task_ids {
                if crate::lifecycle::kill_task(task_id, -1) {
                    stats.killed_tasks = stats.killed_tasks
                                              .saturating_add(1);
                    progress = true;
                }
            }
        }
        for pid in active_impl::collect_exited_process_pids() {
            if let Some(exited) = reap_exited_process(pid) {
                stats.reaped_processes = stats.reaped_processes
                                              .saturating_add(1);
                reaped_tasks.extend(exited);
                progress = true;
            }
        }
        if active_impl::all_process_pids().is_empty() {
            break;
        }
        if !progress {
            break;
        }
    }
    (stats, reaped_tasks)
}

/// 列出 registry 中所有已退出、尚未 reap 的进程 id。
pub fn collect_exited_process_pids() -> Vec<ProcessId> {
    active_impl::collect_exited_process_pids()
}

/// 回收 registry 中所有已退出进程（含 basic 测试遗留、父进程已 reap 的僵尸子进程）。
pub fn reap_all_exited_processes() -> usize {
    let mut total = 0usize;
    loop {
        let pids = active_impl::collect_exited_process_pids();
        if pids.is_empty() {
            break;
        }
        let mut progress = false;
        for pid in pids {
            if reap_exited_process(pid).is_some() {
                total = total.saturating_add(1);
                progress = true;
            }
        }
        if !progress {
            break;
        }
    }
    total
}

/// 回收已退出进程的所有线程 task 与 process registry 记录。
pub fn reap_exited_process(pid : ProcessId) -> Option<Vec<ExitedTask>> {
    let (_process, task_ids) = active_impl::reap_process_with_tasks(pid)?;
    let mut exited = Vec::new();
    for task_id in task_ids {
        if let Some(task) = scheduler::reap_exited_task(task_id) {
            exited.push(task);
        }
    }
    Some(exited)
}

/// 更新当前/指定任务的 clear-child-tid 地址。
pub fn set_task_clear_child_tid(task_id : TaskId, clear_child_tid : Option<TaskClearTid>) -> bool {
    active_impl::set_task_clear_child_tid(task_id, clear_child_tid)
}

/// 读取任务的 clear-child-tid 地址。
pub fn task_clear_child_tid(task_id : TaskId) -> Option<TaskClearTid> {
    active_impl::task_clear_child_tid(task_id)
}

/// 进程 registry 自检。
pub fn process_model_self_test() { active_impl::process_model_self_test(); }

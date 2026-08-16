//! 进程语义、资源限制、wait 事件与进程清理接口。

use alloc::vec::Vec;

use crate::{
    active_impl, scheduler, ExitedTask, ProcessCaps, ProcessId, ProcessResult, ProcessSnapshot,
    ProcessState, ProcessTaskSnapshot, ResourceLimit, TaskClearTid, TaskId, TaskState,
    TaskWaitTarget, ThreadId,
};

/// 查询进程语义快照；第一阶段仅供内部 bring-up / 后续 syscall 迁移使用。
pub fn process_snapshot(pid : ProcessId) -> Option<ProcessSnapshot> {
    active_impl::process_snapshot(pid)
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


/// 查询进程内任务语义快照。
pub fn process_task_snapshot(task_id : TaskId) -> Option<ProcessTaskSnapshot> {
    active_impl::process_task_snapshot(task_id)
}

/// 按用户态 tid 反查调度器内部 task id。
pub fn task_id_for_thread(tid : ThreadId) -> Option<TaskId> { active_impl::task_id_for_thread(tid) }

/// 当前运行任务对应的进程归属快照；未接入真实 spawn 前可能为 `None`。
pub fn current_process_task_snapshot() -> Option<ProcessTaskSnapshot> {
    let task_id = crate::schedule::current_task_id()?;
    process_task_snapshot(task_id)
}

/// 当前运行任务所属进程及其父进程标识。
pub fn current_process_identity() -> Option<(ProcessId, Option<ProcessId>)> {
    let task_id = crate::schedule::current_task_id()?;
    active_impl::process_identity_for_task(task_id)
}

/// 当前运行任务的用户态线程 ID。
pub fn current_thread_id() -> Option<ThreadId> {
    current_process_task_snapshot().map(|snapshot| snapshot.tid)
}

/// 当前运行任务所属进程快照。
pub fn current_process_snapshot() -> Option<ProcessSnapshot> {
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
                                  -> ProcessResult<()> {
    active_impl::set_process_rlimit(pid, resource, limit)
}

/// 查询进程 umask。
pub fn process_umask(pid : ProcessId) -> Option<u32> { active_impl::get_process_umask(pid) }

/// 设置进程 umask。
pub fn set_process_umask(pid : ProcessId, mask : u32) -> ProcessResult<()> {
    active_impl::set_process_umask(pid, mask)
}

/// 查询进程 parent-death-signal。
pub fn process_parent_death_signal(pid : ProcessId) -> Option<i32> {
    active_impl::get_parent_death_signal(pid)
}

/// 设置进程 parent-death-signal。
pub fn set_process_parent_death_signal(pid : ProcessId, sig : i32) -> ProcessResult<()> {
    active_impl::set_parent_death_signal(pid, sig)
}

/// 查询线程名。
pub fn thread_comm(task_id : TaskId) -> Option<[u8; 16]> { active_impl::get_thread_comm(task_id) }

/// 设置线程名。
pub fn set_thread_comm(task_id : TaskId, comm : [u8; 16]) -> ProcessResult<()> {
    active_impl::set_thread_comm(task_id, comm)
}

/// 查询进程组 id。
pub fn process_pgid(pid : ProcessId) -> Option<ProcessId> { active_impl::get_process_pgid(pid) }

/// 设置进程组 id。
pub fn set_process_pgid(pid : ProcessId, pgid : ProcessId) -> ProcessResult<()> {
    active_impl::set_process_pgid(pid, pgid)
}

/// 进程是否仍占 registry 槽位。
pub fn process_exists(pid : ProcessId) -> bool { active_impl::process_exists(pid) }

/// 进程组是否仍有成员。
pub fn pgid_has_members(pgid : ProcessId) -> bool { active_impl::pgid_has_members(pgid) }

/// 返回指定进程组中的全部进程 PID。
pub fn process_pids_in_pgid(pgid : ProcessId) -> Vec<ProcessId> {
    active_impl::process_pids_in_pgid(pgid)
}

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

/// 返回指定进程的全部直接子进程 pid。
pub fn collect_child_pids(pid : ProcessId) -> Vec<ProcessId> {
    active_impl::collect_child_pids(pid)
}

/// 查找当前进程下一个已退出子进程。
pub fn find_exited_child_process(parent_pid : ProcessId) -> Option<ProcessSnapshot> {
    active_impl::find_exited_child_process(parent_pid)
}

/// 查找当前进程在指定进程组内下一个已退出子进程。
pub fn find_exited_child_process_in_pgid(parent_pid : ProcessId,
                                         pgid : ProcessId)
                                         -> Option<ProcessSnapshot> {
    active_impl::find_exited_child_process_in_pgid(parent_pid, pgid)
}

/// 查找父进程下一个 stopped 子进程。
pub fn find_stopped_child_process(parent_pid : ProcessId) -> Option<ProcessSnapshot> {
    active_impl::find_stopped_child_process(parent_pid)
}

/// 指定 stopped 子进程是否可被 wait。
pub fn stopped_child_ready_for_wait(parent_pid : ProcessId,
                                    child_pid : ProcessId)
                                    -> Option<ProcessSnapshot> {
    active_impl::stopped_child_ready_for_wait(parent_pid, child_pid)
}

/// 在 pgid 内查找 stopped 子进程。
pub fn find_stopped_child_process_in_pgid(parent_pid : ProcessId,
                                          pgid : ProcessId)
                                          -> Option<ProcessSnapshot> {
    active_impl::find_stopped_child_process_in_pgid(parent_pid, pgid)
}

/// 查找父进程下一个 continued 子进程。
pub fn find_continued_child_process(parent_pid : ProcessId) -> Option<ProcessSnapshot> {
    active_impl::find_continued_child_process(parent_pid)
}

/// 指定 continued 子进程是否可被 wait。
pub fn continued_child_ready_for_wait(parent_pid : ProcessId,
                                      child_pid : ProcessId)
                                      -> Option<ProcessSnapshot> {
    active_impl::continued_child_ready_for_wait(parent_pid, child_pid)
}

/// 在 pgid 内查找 continued 子进程。
pub fn find_continued_child_process_in_pgid(parent_pid : ProcessId,
                                            pgid : ProcessId)
                                            -> Option<ProcessSnapshot> {
    active_impl::find_continued_child_process_in_pgid(parent_pid, pgid)
}

/// 将进程标为 SIGSTOP 停止态。
pub fn mark_process_stopped(pid : ProcessId, signo : u8) -> ProcessResult<()> {
    active_impl::mark_process_stopped(pid, signo)
}

/// 将进程从 stopped 恢复为 running。
pub fn mark_process_continued(pid : ProcessId) -> ProcessResult<()> {
    active_impl::mark_process_continued(pid)
}

/// 消费 stop 事件的 wait 可见性。
pub fn consume_stop_wait(pid : ProcessId, nowait : bool) {
    active_impl::consume_stop_wait(pid, nowait)
}

/// 消费 continued 事件的 wait 可见性。
pub fn consume_continued_wait(pid : ProcessId, nowait : bool) {
    active_impl::consume_continued_wait(pid, nowait)
}

/// 恢复进程内被 SIGSTOP 挂起的任务（SIGCONT）。
pub fn continue_process_tasks(pid : ProcessId) {
    let Some(task_ids) = task_ids_for_process(pid) else {
        return;
    };
    for task_id in task_ids {
        if scheduler::task_snapshot(task_id).is_some_and(|snapshot| {
                                                snapshot.state ==
                                                TaskState::Blocking(TaskWaitTarget::Manual)
                                            })
        {
            let _ = scheduler::wake_task(task_id);
        }
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
pub fn create_session_for_process(pid : ProcessId) -> ProcessResult<()> {
    active_impl::create_session_for_process(pid)
}

/// 查询进程 dumpable 标志。
pub fn process_dumpable(pid : ProcessId) -> Option<bool> { active_impl::process_dumpable(pid) }

/// 设置进程 dumpable 标志。
pub fn set_process_dumpable(pid : ProcessId, dumpable : bool) -> ProcessResult<()> {
    active_impl::set_process_dumpable(pid, dumpable)
}

/// 查询 child subreaper 标志。
pub fn process_child_subreaper(pid : ProcessId) -> Option<bool> {
    active_impl::process_child_subreaper(pid)
}

/// 设置 child subreaper 标志。
pub fn set_process_child_subreaper(pid : ProcessId, enabled : bool) -> ProcessResult<()> {
    active_impl::set_process_child_subreaper(pid, enabled)
}

/// 查询进程 capability 三集合。
pub fn process_caps(pid : ProcessId) -> Option<ProcessCaps> { active_impl::process_caps(pid) }

/// 设置进程 capability 三集合。
pub fn set_process_caps(pid : ProcessId, caps : ProcessCaps) -> ProcessResult<()> {
    active_impl::set_process_caps(pid, caps)
}

/// 查询进程 KEEPCAPS 标志。
pub fn process_keep_caps(pid : ProcessId) -> Option<bool> { active_impl::process_keep_caps(pid) }

/// 设置进程 KEEPCAPS 标志。
pub fn set_process_keep_caps(pid : ProcessId, enabled : bool) -> ProcessResult<()> {
    active_impl::set_process_keep_caps(pid, enabled)
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
            let Some(snapshot) = active_impl::process_snapshot(*pid) else {
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
    let task_ids = active_impl::task_ids_for_process(pid)?;
    if task_ids.iter()
               .any(|task_id| {
                   !matches!(scheduler::task_state(*task_id),
                             Some(TaskState::Exited(_)))
               })
    {
        return None;
    }
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
pub fn set_task_clear_child_tid(task_id : TaskId,
                                clear_child_tid : Option<TaskClearTid>)
                                -> ProcessResult<()> {
    active_impl::set_task_clear_child_tid(task_id, clear_child_tid)
}

/// 读取任务的 clear-child-tid 地址。
pub fn task_clear_child_tid(task_id : TaskId) -> Option<TaskClearTid> {
    active_impl::task_clear_child_tid(task_id)
}

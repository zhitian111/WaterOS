//! 任务生命周期接口：fork/clone/exec/exit/kill。

use alloc::vec::Vec;

use crate::{
    active_impl, scheduler, AddressSpaceHandle, AddressSpaceRef, CloneFlags, ExitedTask, ProcessId,
    ProcessTaskRole, TaskClearTid, TaskExitCode, TaskId, UserImageInfo, UserStack,
};

pub use impl_core::ParentDeathNotification;

pub struct TaskExitOutcome {
    pub completed_process : Option<ProcessId>,
    pub parent_death_notifications : Vec<ParentDeathNotification>,
}

pub struct KilledTaskOutcome {
    pub killed : bool,
    pub parent_death_notifications : Vec<ParentDeathNotification>,
}

pub struct ExecThreadTermination {
    pub exited_tasks : Vec<ExitedTask>,
    pub parent_death_notifications : Vec<ParentDeathNotification>,
}

/// 从当前用户任务创建一个尚未进入就绪队列的 fork 子任务。
pub fn fork_current(child_stack : usize,
                    new_aspace_ptr : usize,
                    new_satp : usize)
                    -> Option<TaskId> {
    fork_current_parented(child_stack, new_aspace_ptr, new_satp, false)
}

/// fork 子进程；`clone_parent` 为真时，新进程与调用进程成为兄弟。
pub fn fork_current_parented(child_stack : usize,
                             new_aspace_ptr : usize,
                             new_satp : usize,
                             clone_parent : bool)
                             -> Option<TaskId> {
    let parent_task = crate::process::current_process_task_snapshot()?;
    let parent_pid = parent_task.pid;
    let parent_process = crate::process::process_snapshot(parent_pid)?;
    let child_parent_pid = if clone_parent {
        parent_process.parent_pid?
    } else {
        parent_pid
    };
    let child_parent_leader = crate::process::process_snapshot(child_parent_pid)?.leader_task_id;
    let child_id = scheduler::create_fork_child(child_stack,
                                                new_aspace_ptr,
                                                new_satp,
                                                child_parent_leader)?;
    let address_space = Some(AddressSpaceRef::new(AddressSpaceHandle::from_raw(new_satp),
                                                  new_aspace_ptr));
    let registered = active_impl::with_process_registry(|registry| {
        registry.create_process_like_fork_with_parent(parent_pid,
                                                      child_parent_pid,
                                                      parent_task.task_id,
                                                      child_id,
                                                      address_space)
    });
    if registered.is_err() {
        scheduler::discard_unstarted_task(child_id);
        return None;
    }
    Some(child_id)
}

/// 创建临时共享父进程地址空间的 vfork 子进程。
/// 子进程不持有该地址空间的所有权；在子进程 exec 或退出前，地址空间始终由父进程所有。
pub fn vfork_current(child_stack : usize,
                     shared_aspace_ptr : usize,
                     shared_satp : usize)
                     -> Option<TaskId> {
    vfork_current_parented(child_stack, shared_aspace_ptr, shared_satp, false)
}

/// vfork 版本的 `CLONE_PARENT` 语义；地址空间仍从当前调用者共享。
pub fn vfork_current_parented(child_stack : usize,
                              shared_aspace_ptr : usize,
                              shared_satp : usize,
                              clone_parent : bool)
                              -> Option<TaskId> {
    let parent_task = crate::process::current_process_task_snapshot()?;
    let parent_pid = parent_task.pid;
    let parent_process = crate::process::process_snapshot(parent_pid)?;
    let child_parent_pid = if clone_parent {
        parent_process.parent_pid?
    } else {
        parent_pid
    };
    let child_parent_leader = crate::process::process_snapshot(child_parent_pid)?.leader_task_id;
    let child_id = scheduler::create_fork_child(child_stack,
                                                shared_aspace_ptr,
                                                shared_satp,
                                                child_parent_leader)?;
    let registered = active_impl::with_process_registry(|registry| {
        registry.create_process_like_fork_with_parent(parent_pid,
                                                      child_parent_pid,
                                                      parent_task.task_id,
                                                      child_id,
                                                      None)
    });
    if registered.is_err() {
        scheduler::discard_unstarted_task(child_id);
        return None;
    }
    Some(child_id)
}

/// 完成 fork 资源继承后，将子任务发布到调度器。
pub fn start_fork_child(child_id : TaskId) { scheduler::enqueue_ready_task(child_id); }

/// fork 失败回滚：撤销子任务 TCB、进程槽位与地址空间；返回被撤销的子进程 PID（供信号表清理）。
pub fn abort_fork_child(child_id : TaskId) -> Option<ProcessId> {
    scheduler::discard_unstarted_task(child_id);
    active_impl::abort_forked_process(child_id).ok()
}

/// clone 线程失败回滚：撤销子线程 TCB 与进程内线程登记。
pub fn abort_clone_thread(child_id : TaskId) {
    scheduler::discard_unstarted_task(child_id);
    let _ = active_impl::abort_cloned_thread(child_id);
}

/// 从当前用户任务 clone 一个同进程线程并登记到当前进程，但暂不发布到就绪队列。
///
/// syscall 层必须先完成 signal、credential、VFS 等线程侧表的继承，再调用
/// [`start_clone_thread`]；否则其他 CPU 可能在侧表初始化完成前运行子线程。
pub fn clone_current_thread(child_stack : usize,
                            tls : usize,
                            clone_flags : CloneFlags,
                            clear_child_tid : Option<TaskClearTid>)
                            -> Option<TaskId> {
    let process_task = crate::process::current_process_task_snapshot()?;
    let child_id = scheduler::create_clone_thread(child_stack,
                                                  tls,
                                                  clone_flags.contains(CloneFlags::CLONE_SETTLS))?;
    let registered = active_impl::with_process_registry(|registry| {
        registry.add_task_to_process(process_task.pid,
                                     process_task.task_id,
                                     child_id,
                                     clone_flags,
                                     tls,
                                     clear_child_tid)
    });
    if registered.is_err() {
        scheduler::discard_unstarted_task(child_id);
        return None;
    }
    Some(child_id)
}

/// 完成 clone 线程的资源继承后，将子线程发布到调度器。
pub fn start_clone_thread(child_id : TaskId) { scheduler::enqueue_ready_task(child_id); }

/// execve：替换当前任务的进程映像。
pub fn execve_current(entry_pc : usize,
                      sp : usize,
                      argc : usize,
                      argv : usize,
                      envp : usize,
                      satp : usize,
                      user_aspace_ptr : usize,
                      image_info : UserImageInfo,
                      stack_info : UserStack) {
    let current_pid = crate::process::current_process_task_snapshot().map(|task| task.pid);
    scheduler::execve_current(entry_pc,
                              sp,
                              argc,
                              argv,
                              envp,
                              satp,
                              user_aspace_ptr,
                              image_info,
                              stack_info);
    if let Some(pid) = current_pid {
        active_impl::with_process_registry(|registry| {
            let _ = registry.update_process_address_space(
                pid,
                Some(AddressSpaceRef::new(
                    AddressSpaceHandle::from_raw(satp),
                    user_aspace_ptr,
                )),
            );
        });
    }
}

/// 标记当前任务退出；若本次操作使整个进程完成退出，则返回该进程 PID。
pub fn record_current_task_exit(exit_code : TaskExitCode) -> TaskExitOutcome {
    let Some(task_id) = crate::schedule::current_task_id() else {
        return TaskExitOutcome { completed_process : None,
                                 parent_death_notifications : Vec::new() };
    };
    let Some(process_task) = active_impl::process_task_snapshot(task_id) else {
        return TaskExitOutcome { completed_process : None,
                                 parent_death_notifications : Vec::new() };
    };
    match active_impl::with_process_registry(|registry| {
              registry.mark_task_exited(task_id, exit_code)
          }) {
        Ok(result) => TaskExitOutcome { completed_process : result.process_completed
                                                                  .then_some(process_task.pid),
                                        parent_death_notifications:
                                            result.parent_death_notifications },
        Err(_) => TaskExitOutcome { completed_process : None,
                                    parent_death_notifications : Vec::new() },
    }
}

/// 让当前任务以给定退出码结束运行。
pub fn exit_current(exit_code : TaskExitCode) -> ! {
    let _ = record_current_task_exit(exit_code);
    scheduler::exit_current(exit_code)
}

/// Publish process-wide exit before notifying or rescheduling sibling tasks.
pub fn begin_current_process_exit(exit_code : TaskExitCode) -> Vec<ParentDeathNotification> {
    if let Some(process_task) = crate::process::current_process_task_snapshot() {
        return active_impl::with_process_registry(|registry| {
            registry.mark_process_exited(process_task.pid, exit_code)
                    .unwrap_or_default()
        });
    }
    Vec::new()
}

/// 在进程注册表中记录当前进程退出，供退出通知前建立可见性顺序。
pub fn record_current_process_exit(exit_code : TaskExitCode) {
    begin_current_process_exit(exit_code);
    let _ = record_current_task_exit(exit_code);
}

/// 以 exit_group 语义终止当前进程内所有线程。
pub fn exit_group_current(exit_code : TaskExitCode) -> ! {
    let current_id =
        crate::schedule::current_task_id().expect("exit_group requires a current task");
    if let Some(process_task) = crate::process::current_process_task_snapshot() {
        let task_ids = active_impl::task_ids_for_process(process_task.pid).unwrap_or_default();
        begin_current_process_exit(exit_code);
        for task_id in task_ids {
            if task_id != current_id {
                let _ = kill_task(task_id, exit_code);
            }
        }
    }
    let _ = record_current_task_exit(exit_code);
    scheduler::exit_current(exit_code)
}

/// 终止指定任务（非当前任务）。
pub fn kill_task_with_notifications(task_id : TaskId,
                                    exit_code : TaskExitCode)
                                    -> KilledTaskOutcome {
    let killed = scheduler::kill_task(task_id, exit_code);
    let parent_death_notifications = if killed {
        active_impl::with_process_registry(|registry| {
            registry.mark_task_exited(task_id, exit_code)
                    .map(|result| result.parent_death_notifications)
                    .unwrap_or_default()
        })
    } else {
        Vec::new()
    };
    KilledTaskOutcome { killed,
                        parent_death_notifications }
}

pub fn kill_task(task_id : TaskId, exit_code : TaskExitCode) -> bool {
    kill_task_with_notifications(task_id, exit_code).killed
}

/// execve 前清理同进程其它线程；当前保守实现要求多线程 exec 由 leader 发起。
pub fn terminate_other_threads_for_exec() -> Result<ExecThreadTermination, ()> {
    let current_id = crate::schedule::current_task_id().ok_or(())?;
    let process_task = crate::process::current_process_task_snapshot().ok_or(())?;
    if process_task.role != ProcessTaskRole::Leader {
        return Err(());
    }

    let task_ids = active_impl::with_process_registry(|registry| {
                       registry.begin_process_exec(process_task.pid, current_id)
                   }).map_err(|_| ())?;
    let mut pending = task_ids.into_iter()
                              .filter(|task_id| *task_id != current_id)
                              .collect::<Vec<_>>();
    let mut reaped = Vec::new();
    let mut notifications = Vec::new();
    while !pending.is_empty() {
        let mut still_running = Vec::new();
        for task_id in pending {
            let outcome = kill_task_with_notifications(task_id, 0);
            notifications.extend(outcome.parent_death_notifications);
            if outcome.killed {
                if let Some(exited) = scheduler::reap_exited_task(task_id) {
                    reaped.push(exited);
                }
            } else if let Some(exited) = scheduler::reap_exited_task(task_id) {
                reaped.push(exited);
            } else if scheduler::task_state(task_id).is_some() {
                scheduler::request_task_reschedule(task_id);
                still_running.push(task_id);
            }
        }
        pending = still_running;
        if !pending.is_empty() {
            crate::schedule::yield_now();
        }
    }
    active_impl::with_process_registry(|registry| {
        let _ = registry.retain_only_task_in_process(process_task.pid, current_id);
    });
    Ok(ExecThreadTermination { exited_tasks : reaped,
                               parent_death_notifications : notifications })
}

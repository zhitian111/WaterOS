//! 任务生命周期接口：fork/clone/exec/exit/kill。

use alloc::vec::Vec;

use crate::{
    active_impl, scheduler, AddressSpaceHandle, AddressSpaceRef, CloneFlags, ExitedTask, ProcessId,
    ProcessTaskRole, TaskClearTid, TaskExitCode, TaskId, UserImageInfo, UserStack,
};

/// 从当前用户任务创建一个尚未进入就绪队列的 fork 子任务。
pub fn fork_current(child_stack: usize, new_aspace_ptr: usize, new_satp: usize) -> Option<TaskId> {
    let parent_pid = crate::process::current_process_task_snapshot().map(|task| task.pid)?;
    let parent_leader = crate::process::process_snapshot(parent_pid)?.leader_task_id;
    let child_id =
        scheduler::create_fork_child(child_stack, new_aspace_ptr, new_satp, parent_leader)?;
    let address_space = Some(AddressSpaceRef::new(
        AddressSpaceHandle::from_raw(new_satp),
        new_aspace_ptr,
    ));
    let registered = active_impl::with_process_registry(|registry| {
        registry.create_process_like_fork(parent_pid, child_id, address_space)
    });
    if registered.is_err() {
        scheduler::discard_unstarted_task(child_id);
        return None;
    }
    Some(child_id)
}

/// 完成 fork 资源继承后，将子任务发布到调度器。
pub fn start_fork_child(child_id: TaskId) {
    scheduler::enqueue_ready_task(child_id);
}

/// fork 失败回滚：撤销子任务 TCB、进程槽位与地址空间；返回被撤销的子进程 PID（供信号表清理）。
pub fn abort_fork_child(child_id: TaskId) -> Option<ProcessId> {
    scheduler::discard_unstarted_task(child_id);
    active_impl::abort_forked_process(child_id).ok()
}

/// clone 线程失败回滚：撤销子线程 TCB 与进程内线程登记。
pub fn abort_clone_thread(child_id: TaskId) {
    scheduler::discard_unstarted_task(child_id);
    let _ = active_impl::abort_cloned_thread(child_id);
}

/// 从当前用户任务 clone 一个同进程线程并登记到当前进程，但暂不发布到就绪队列。
///
/// syscall 层必须先完成 signal、credential、VFS 等线程侧表的继承，再调用
/// [`start_clone_thread`]；否则其他 CPU 可能在侧表初始化完成前运行子线程。
pub fn clone_current_thread(
    child_stack: usize,
    tls: usize,
    clone_flags: CloneFlags,
    clear_child_tid: Option<TaskClearTid>,
) -> Option<TaskId> {
    let process_task = crate::process::current_process_task_snapshot()?;
    let child_id = scheduler::create_clone_thread(
        child_stack,
        tls,
        clone_flags.contains(CloneFlags::CLONE_SETTLS),
    )?;
    let registered = active_impl::with_process_registry(|registry| {
        registry.add_task_to_process(
            process_task.pid,
            child_id,
            clone_flags,
            tls,
            clear_child_tid,
        )
    });
    if registered.is_err() {
        scheduler::discard_unstarted_task(child_id);
        return None;
    }
    Some(child_id)
}

/// 完成 clone 线程的资源继承后，将子线程发布到调度器。
pub fn start_clone_thread(child_id: TaskId) {
    scheduler::enqueue_ready_task(child_id);
}

/// execve：替换当前任务的进程映像。
pub fn execve_current(
    entry_pc: usize,
    sp: usize,
    argc: usize,
    argv: usize,
    envp: usize,
    satp: usize,
    user_aspace_ptr: usize,
    image_info: UserImageInfo,
    stack_info: UserStack,
) {
    let current_pid = crate::process::current_process_task_snapshot().map(|task| task.pid);
    scheduler::execve_current(
        entry_pc,
        sp,
        argc,
        argv,
        envp,
        satp,
        user_aspace_ptr,
        image_info,
        stack_info,
    );
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

/// 让当前任务以给定退出码结束运行。
pub fn record_current_task_exit(exit_code: TaskExitCode) {
    if let Some(task_id) = crate::schedule::current_task_id() {
        active_impl::with_process_registry(|registry| {
            let _ = registry.mark_task_exited(task_id, exit_code);
        });
    }
}

/// 让当前任务以给定退出码结束运行。
pub fn exit_current(exit_code: TaskExitCode) -> ! {
    record_current_task_exit(exit_code);
    scheduler::exit_current(exit_code)
}

/// 在进程注册表中记录当前进程退出，供退出通知前建立可见性顺序。
pub fn record_current_process_exit(exit_code: TaskExitCode) {
    if let Some(process_task) = crate::process::current_process_task_snapshot() {
        active_impl::with_process_registry(|registry| {
            let _ = registry.mark_process_exited(process_task.pid, exit_code);
        });
    }
}

/// 以 exit_group 语义终止当前进程内所有线程。
pub fn exit_group_current(exit_code: TaskExitCode) -> ! {
    let current_id =
        crate::schedule::current_task_id().expect("exit_group requires a current task");
    if let Some(process_task) = crate::process::current_process_task_snapshot() {
        let task_ids = active_impl::task_ids_for_process(process_task.pid).unwrap_or_default();
        record_current_process_exit(exit_code);
        for task_id in task_ids {
            if task_id != current_id {
                let _ = scheduler::kill_task(task_id, exit_code);
            }
        }
    }
    scheduler::exit_current(exit_code)
}

/// 终止指定任务（非当前任务）。
pub fn kill_task(task_id: TaskId, exit_code: TaskExitCode) -> bool {
    let killed = scheduler::kill_task(task_id, exit_code);
    if killed {
        active_impl::with_process_registry(|registry| {
            let _ = registry.mark_task_exited(task_id, exit_code);
        });
    }
    killed
}

/// execve 前清理同进程其它线程；当前保守实现要求多线程 exec 由 leader 发起。
pub fn terminate_other_threads_for_exec() -> Result<Vec<ExitedTask>, ()> {
    let current_id = crate::schedule::current_task_id().ok_or(())?;
    let process_task = crate::process::current_process_task_snapshot().ok_or(())?;
    let process = crate::process::process_snapshot(process_task.pid).ok_or(())?;
    if process.task_count <= 1 {
        return Ok(Vec::new());
    }
    if process_task.role != ProcessTaskRole::Leader {
        return Err(());
    }

    let task_ids = active_impl::task_ids_for_process(process_task.pid).ok_or(())?;
    let mut reaped = Vec::new();
    for task_id in task_ids {
        if task_id == current_id {
            continue;
        }
        if kill_task(task_id, 0) {
            if let Some(exited) = scheduler::reap_exited_task(task_id) {
                reaped.push(exited);
            }
        }
    }
    active_impl::with_process_registry(|registry| {
        let _ = registry.retain_only_task_in_process(process_task.pid, current_id);
    });
    Ok(reaped)
}

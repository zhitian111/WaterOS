#![no_std]

extern crate alloc;
mod cpu;
mod lifecycle;
mod process;
mod runtime;
pub mod sched;
mod schedule;
mod spawn;
mod trap;
pub mod wait_queue;
pub use self::wait_queue::WaitQueue;
pub use api_v0::CpuMask;
pub use lifecycle::{
    abort_clone_thread, abort_fork_child, begin_current_process_exit, clone_current_thread,
    execve_current, exit_current, exit_group_current, fork_current, kill_task,
    kill_task_with_notifications, record_current_process_exit, record_current_task_exit,
    start_clone_thread, start_fork_child, terminate_other_threads_for_exec, vfork_current,
    ExecThreadTermination, KilledTaskOutcome, ParentDeathNotification, TaskExitOutcome,
};

pub use process::{
    all_process_pids, collect_child_pids, collect_exited_process_pids, consume_continued_wait,
    consume_stop_wait, continue_process_tasks, continued_child_ready_for_wait,
    create_session_for_process, current_process_identity, current_process_snapshot,
    current_process_task_snapshot, current_thread_id, find_continued_child_process,
    find_continued_child_process_in_pgid, find_exited_child_process,
    find_exited_child_process_in_pgid, find_stopped_child_process,
    find_stopped_child_process_in_pgid, has_child_process, has_child_process_in_pgid,
    leader_task_for_process, mark_process_continued, mark_process_stopped, nofile_rlimit_for_task,
    pgid_has_members, process_child_subreaper, process_dumpable, process_exists,
    process_parent_death_signal, process_pgid, process_pids_in_pgid, process_resource_limit,
    process_snapshot, process_task_snapshot, process_umask, purge_all_user_processes,
    reap_all_exited_processes, reap_exited_member_threads, reap_exited_process,
    set_process_child_subreaper, set_process_dumpable, set_process_parent_death_signal,
    set_process_pgid, set_process_resource_limit, set_process_umask, set_task_clear_child_tid,
    set_thread_comm, stopped_child_ready_for_wait, task_clear_child_tid, task_id_for_thread,
    task_ids_for_process, thread_comm, wake_parent_child_waiters, ProcessPurgeStats,
};
pub use sched::{
    cpu_affinity_ret_bytes, get_affinity, get_nice, get_param, get_scheduler_policy,
    resolve_sched_pid, set_affinity, set_nice, set_param, set_scheduler_policy,
    validate_cpu_affinity_buf_len,
};
pub use schedule::{
    block_current, current_task_id, current_task_snapshot, current_task_user_aspace_ptr,
    current_tick, interrupt_task, log_stall_diagnostics, reap_exited_task, schedule_reschedule,
    schedule_tick, sleep_for_ticks, task_snapshot, task_state, wait_for_task_exit,
    wait_for_task_exit_for_ticks, wait_on, wait_on_for_ticks, wait_on_while,
    wait_on_while_for_ticks, wake_task, yield_now,
};
pub use spawn::{
    create_user_task, spawn_kernel_task, spawn_user_task, spawn_user_task_from_loaded_elf,
    start_user_task, user_task_from_loaded_elf,
};
pub use trap::{begin_current_trap_frame_access, restore_current_trap_frame};
mod scheduler {
    pub use scheduler::*;
}
pub use api_v0::{
    AddressSpaceHandle, AddressSpaceRef, CloneFlags, CpuId, KernelTaskEntry, ProcessError,
    ProcessId, ProcessResult, ProcessSnapshot, ProcessState, ProcessTaskRole, ProcessTaskSnapshot,
    ProcessTaskState, ResourceLimit, SchedError, SchedPolicy, TaskClearTid, TaskExitCode,
    TaskSnapshot, TaskState, TaskTick, TaskWaitResult, TaskWaitTarget, ThreadId, UserImageInfo,
    UserStack, UserTask, WaitQueueId,
};
pub use api_v0::{ExitedTask, TaskId, TaskKind};
pub use cpu::{
    cpu_snapshot, cpu_states, online_cpu_mask, print_cpu_states, request_task_reschedule,
    running_cpu, set_cpu_online, set_timekeeper_cpu, total_idle_ticks,
};
pub(crate) use impl_core as active_impl;
pub use scheduler::CpuSnapshot;

// ============================================================================
// 与主函数的接口
// ============================================================================
/// 初始化任务系统和底层调度器状态。
pub fn init() {
    scheduler::init();
    active_impl::init_process_registry();
}

#[cfg(feature = "self_test")]
/// 任务组件内核态自检；只验证调度器已建立，不创建或切换用户任务。
pub fn self_test() {
    log::info!("[task] self_test begin");
    let mask = online_cpu_mask();
    assert!(mask.bits() != 0, "at least the boot CPU must be online");
    let idle_ticks = total_idle_ticks();
    log::info!("[task] self_test observed idle_ticks={}", idle_ticks);
    log::info!("[task] self_test complete; no task state was mutated");
}
/// 启动调度器并切入第一批可运行任务。
pub fn run_first_task() -> ! { scheduler::run_first_task() }

//! WaterOS 任务子系统聚合 crate：对上暴露稳定 API，对下组合 **任务数据模型** 与
//! **调度实现**。
//!
//! ## 职责划分
//!
//! - [`api`]（`wateros-task-api-v0`）：任务 ID、状态、等待句柄、用户任务规格等
//!   **跨层语义类型**；不含调度策略与 per-task 内存布局。
//! - [`scheduler`]（`wateros-task-scheduler`）：**何时运行谁**——就绪队列、阻塞/
//!   睡眠/等待、tick 与主动让出、上下文切换入口；通过 `active_impl`
//!   绑定具体算法（如轮转）。
//! - **`impl-core`**（`wateros-task-impl-core`，feature
//!   `impl-core`）：**单个任务长什么样**——`TaskControlBlock`、内核/用户栈、trap
//!   现场与用户态镜像的装配；供调度器实现持有并驱动切换，本 crate 通过
//!   [`crate::active_impl::TaskBootstrap`] 等再导出给 arch 入口。
//!
//! 本文件中的 `spawn`/`yield`/`wait` 等函数是对 `scheduler`
//! 的薄封装；私有 `runtime` 提供 trap 返回路径的 Rust 入口及与汇编/switch
//! 约定的 `extern "C"` 任务入口符号；组合层 `trap_handler::init` 注册
//! `arch-api::kernel_trap` 后由 `trap_entry_rust` 转入。
//!
//! ## 后续替换点
//!
//! 更换调度算法时改 `task-scheduler` 的 `active_impl`；更换 TCB/栈布局时改
//! `impl-core`。二者边界应保持：**调度器不定义 TCB 字段布局，impl-core
//! 不决定全局就绪顺序**。

#![no_std]

extern crate alloc;

use alloc::vec::Vec;

mod runtime;
pub mod wait_queue;
pub use self::wait_queue::WaitQueue;
mod scheduler {
    pub use scheduler::*;
}
pub use api_v0::{
    AddressSpaceHandle, AddressSpaceRef, CloneFlags, CwdRef, FileTableRef, KernelTaskEntry,
    ProcessDescriptor, ProcessId, ProcessState, ResourceHandle, SignalHandlersRef, TaskBlockReason,
    TaskClearTid, TaskExitCode, TaskGroupId, TaskSnapshot, TaskTick, TaskWaitResult,
    ProcessTaskDescriptor, ProcessTaskRole, ProcessTaskState, UserImageInfo, UserStack, UserTask,
    WaitQueueId,
};
pub use api_v0::{ExitedTask, TaskId, TaskKind, TaskWaitHandle};
#[cfg(feature = "impl-core")]
pub(crate) use impl_core as active_impl;
use mm_api::kernel_bringup::LoadedElf;

/// 初始化任务系统和底层调度器状态。
#[inline]
pub fn init() {
    active_impl::init_process_registry();
    active_impl::process_model_self_test();
    scheduler::init();
}

/// Trap handler 进入时，把栈上 trap frame
/// 交给当前任务保存区，并返回后续应修改的权威 frame。
#[inline]
pub unsafe fn begin_current_trap_frame_access(frame : *mut u8) -> *mut u8 {
    unsafe { crate::runtime::begin_current_trap_frame_access(frame) }
}

/// Trap handler 返回前，把当前任务保存区的 trap frame 写回栈上
/// frame，并准备返回地址空间 token。
#[inline]
pub unsafe fn restore_current_trap_frame(frame : *mut u8) -> bool {
    unsafe { crate::runtime::restore_current_trap_frame(frame) }
}

/// 创建一个新的内核任务，并返回分配到的任务号。
#[inline]
pub fn spawn_kernel_task(entry : KernelTaskEntry, arg : usize) -> TaskId {
    scheduler::spawn_kernel_task(entry, arg)
}

/// 按给定规格创建一个新的用户任务，并返回分配到的任务号。
#[inline]
pub fn spawn_user_task(user : UserTask) -> TaskId {
    let task_id = scheduler::spawn_user_task(user);
    let parent_pid = current_process_task_snapshot().map(|task| task.pid);
    let address_space = user_address_space_ref(user);
    active_impl::with_process_registry(|registry| {
        registry.create_process_for_task(task_id, parent_pid, address_space);
    });
    task_id
}

/// 兼容旧调用点的用户任务创建别名。
#[inline]
pub fn spawn_user_task_spec(user : UserTask) -> TaskId { spawn_user_task(user) }

/// 将 MM ELF loader 产出的地址空间、映像与外部用户栈元数据转换为用户任务规格。
#[inline]
pub fn user_task_from_loaded_elf(loaded : &LoadedElf) -> UserTask {
    UserTask::new(loaded.entry_pc,
                  AddressSpaceHandle::from_raw(loaded.satp),
                  UserImageInfo::new(loaded.image_base, loaded.image_size),
                  UserStack::from_range(loaded.stack_bottom, loaded.stack_top),
                  loaded.user_aspace_ptr)
}

/// 基于 MM 已装载的 ELF 创建一个用户任务，并返回分配到的任务号。
#[inline]
pub fn spawn_user_task_from_loaded_elf(loaded : &LoadedElf) -> TaskId {
    spawn_user_task(user_task_from_loaded_elf(loaded))
}

/// 启动调度器并切入第一批可运行任务。
#[inline]
pub fn run_first_task() -> ! { scheduler::run_first_task() }

/// 让当前任务主动让出 CPU。
#[inline]
pub fn yield_now() { scheduler::suspend_current_and_run_next(); }

/// 通知任务系统发生了一次时钟 tick。
#[inline]
pub fn schedule_tick() { scheduler::schedule_tick(); }

/// 以指定阻塞原因挂起当前任务。
#[inline]
pub fn block_current(reason : TaskBlockReason) { scheduler::block_current(reason); }

/// 让当前任务等待指定的阻塞对象。
#[inline]
pub fn wait_on(wait_handle : TaskWaitHandle) { scheduler::wait_current(wait_handle); }

/// 在调度临界区内复查条件；条件仍成立才等待指定的阻塞对象。
#[inline]
pub fn wait_on_while(wait_handle : TaskWaitHandle, condition : impl FnOnce() -> bool) {
    scheduler::wait_current_while(wait_handle, condition);
}

/// 让当前任务等待指定的阻塞对象，并带一个超时。
#[inline]
pub fn wait_on_for_ticks(wait_handle : TaskWaitHandle, timeout_ticks : TaskTick) -> TaskWaitResult {
    scheduler::wait_current_timeout(wait_handle, timeout_ticks)
}

/// 在调度临界区内复查条件；条件仍成立才带超时等待指定阻塞对象。
#[inline]
pub fn wait_on_while_for_ticks(wait_handle : TaskWaitHandle,
                               timeout_ticks : TaskTick,
                               condition : impl FnOnce() -> bool)
                               -> TaskWaitResult {
    scheduler::wait_current_timeout_while(wait_handle, timeout_ticks, condition)
}

/// 返回“等待指定任务退出”的通用等待句柄。
#[inline]
pub const fn task_exit_wait_handle(task_id : TaskId) -> TaskWaitHandle {
    TaskWaitHandle::for_task_exit(task_id)
}

/// 让当前任务等待指定任务退出。
#[inline]
pub fn wait_for_task_exit(task_id : TaskId) { wait_on(task_exit_wait_handle(task_id)); }

/// 让当前任务等待指定任务退出，并带一个超时。
#[inline]
pub fn wait_for_task_exit_for_ticks(task_id : TaskId, timeout_ticks : TaskTick) -> TaskWaitResult {
    wait_on_for_ticks(task_exit_wait_handle(task_id),
                      timeout_ticks)
}

/// 让当前任务睡眠指定数量的 tick。
#[inline]
pub fn sleep_for_ticks(ticks : TaskTick) { scheduler::sleep_current_for_ticks(ticks); }

/// 尝试唤醒指定任务。
#[inline]
pub fn wake_task(task_id : TaskId) -> bool { scheduler::wake_task(task_id) }

/// 回收指定已退出任务的信息。
#[inline]
pub fn reap_exited_task(task_id : TaskId) -> Option<ExitedTask> {
    let leader_pid = active_impl::lookup_task(task_id).and_then(|process_task| {
        let process = active_impl::lookup_process(process_task.pid)?;
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

/// 从当前用户任务 fork 一个子任务，并返回子任务 id。
///
/// 子任务获得父任务 trap 帧副本（a0 置 0），使用独立地址空间。
/// `child_stack` 非零时，子任务初始用户栈指针设为该值（用于 clone 新栈场景）。
/// `new_aspace_ptr` / `new_satp` 由 `mm::kernel_mm::fork_user_aspace()` 提供。
/// 无当前任务或当前不是用户任务时返回 `None`。
#[inline]
pub fn fork_current(child_stack : usize,
                    new_aspace_ptr : usize,
                    new_satp : usize)
                    -> Option<TaskId> {
    let parent_pid = current_process_task_snapshot().map(|task| task.pid)?;
    let child_id = scheduler::fork_current(child_stack, new_aspace_ptr, new_satp)?;
    let address_space =
        Some(AddressSpaceRef::new(AddressSpaceHandle::from_raw(new_satp), new_aspace_ptr));
    active_impl::with_process_registry(|registry| {
        let _ = registry.create_process_like_fork(parent_pid, child_id, address_space);
    });
    Some(child_id)
}

/// 从当前用户任务 clone 一个同进程线程，并登记到当前进程。
#[inline]
pub fn clone_current_thread(child_stack : usize,
                            tls : usize,
                            clone_flags : CloneFlags,
                            clear_child_tid : Option<TaskClearTid>)
                            -> Option<TaskId> {
    let process_task = current_process_task_snapshot()?;
    let child_id = scheduler::clone_current_thread(child_stack,
                                                   tls,
                                                   clone_flags.contains(CloneFlags::CLONE_SETTLS))?;
    active_impl::with_process_registry(|registry| {
        let _ = registry.add_task_to_process(process_task.pid,
                                             child_id,
                                             clone_flags,
                                             tls,
                                             clear_child_tid);
    });
    Some(child_id)
}

/// execve：替换当前任务的进程映像。
#[inline]
pub fn execve_current(entry_pc : usize,
                      sp : usize,
                      argc : usize,
                      argv : usize,
                      envp : usize,
                      satp : usize,
                      user_aspace_ptr : usize,
                      image_info : UserImageInfo,
                      stack_info : UserStack) {
    let current_pid = current_process_task_snapshot().map(|task| task.pid);
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
                Some(AddressSpaceRef::new(AddressSpaceHandle::from_raw(satp), user_aspace_ptr)),
            );
        });
    }
}

/// 回收一个任意已退出任务的信息。
#[inline]
pub fn reap_one_exited_task() -> Option<ExitedTask> {
    let exited = scheduler::reap_one_exited_task()?;
    if let Some(process_task) = active_impl::lookup_task(exited.id) {
        if active_impl::lookup_process(process_task.pid).is_some_and(|process| {
            process.leader_task_id == exited.id && matches!(process.state, ProcessState::Exited(_))
        }) {
            let _ = active_impl::reap_process(process_task.pid);
        }
    }
    Some(exited)
}

/// 回收指定父任务下任意一个已退出子任务的信息。
#[inline]
pub fn reap_one_exited_child(parent_id : TaskId) -> Option<ExitedTask> {
    scheduler::reap_one_exited_child(parent_id)
}

/// 判断指定任务是否仍有子任务。
#[inline]
pub fn has_child(parent_id : TaskId) -> bool { scheduler::has_child(parent_id) }

/// 让当前任务以给定退出码结束运行。
#[inline]
pub fn exit_current(exit_code : TaskExitCode) -> ! {
    if let Some(task_id) = current_task_id() {
        active_impl::with_process_registry(|registry| {
            let _ = registry.mark_task_exited(task_id, exit_code);
        });
    }
    scheduler::exit_current(exit_code)
}

/// 以 exit_group 语义终止当前进程内所有线程。
#[inline]
pub fn exit_group_current(exit_code : TaskExitCode) -> ! {
    let current_id = current_task_id().expect("exit_group requires a current task");
    if let Some(process_task) = current_process_task_snapshot() {
        let task_ids = active_impl::task_ids_for_process(process_task.pid).unwrap_or_default();
        active_impl::with_process_registry(|registry| {
            let _ = registry.mark_process_exiting(process_task.pid, exit_code);
        });
        for task_id in task_ids {
            if task_id != current_id {
                let _ = scheduler::kill_task(task_id, exit_code);
            }
        }
    }
    scheduler::exit_current(exit_code)
}

/// 终止指定任务（非当前任务）。
#[inline]
pub fn kill_task(task_id : TaskId, exit_code : TaskExitCode) -> bool {
    let killed = scheduler::kill_task(task_id, exit_code);
    if killed {
        active_impl::with_process_registry(|registry| {
            let _ = registry.mark_task_exited(task_id, exit_code);
        });
    }
    killed
}

/// execve 前清理同进程其它线程；当前保守实现要求多线程 exec 由 leader 发起。
#[inline]
pub fn terminate_other_threads_for_exec() -> Result<Vec<ExitedTask>, ()> {
    let current_id = current_task_id().ok_or(())?;
    let process_task = current_process_task_snapshot().ok_or(())?;
    let process = process_snapshot(process_task.pid).ok_or(())?;
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

/// 返回当前正在运行任务的任务号。
#[inline]
pub fn current_task_id() -> Option<TaskId> { scheduler::current_task_id() }

/// 返回当前正在运行任务的稳定快照。
#[inline]
pub fn current_task_snapshot() -> Option<TaskSnapshot> { scheduler::current_task_snapshot() }

/// 返回指定任务的稳定快照；任务不存在或已被回收时返回 `None`。
#[inline]
pub fn task_snapshot(task_id : TaskId) -> Option<TaskSnapshot> { scheduler::task_snapshot(task_id) }

/// 返回当前调度器逻辑 tick。
#[inline]
pub fn current_tick() -> TaskTick { scheduler::current_tick() }
#[inline]
pub fn current_task_user_aspace_ptr() -> usize { scheduler::current_task_user_aspace_ptr() }
#[inline]
pub fn current_task_user_address_space_token() -> usize {
    scheduler::current_task_user_address_space_token()
}
#[inline]
pub fn current_task_trap_return_address_space_token() -> usize {
    scheduler::current_task_trap_return_address_space_token()
}

#[inline]
fn user_address_space_ref(user : UserTask) -> Option<AddressSpaceRef> {
    Some(AddressSpaceRef::new(user.address_space()?, user.user_aspace_ptr()?))
}

/// 查询进程语义快照；第一阶段仅供内部 bring-up / 后续 syscall 迁移使用。
#[inline]
pub fn process_snapshot(pid : ProcessId) -> Option<ProcessDescriptor> {
    active_impl::lookup_process(pid)
}

/// 返回 registry 中全部进程 PID（含未 reap 的 zombie）。
#[inline]
pub fn all_process_pids() -> Vec<ProcessId> {
    active_impl::all_process_pids()
}

/// 返回进程内全部调度实体 `TaskId`（供 syscall robust 清理等路径使用）。
#[inline]
pub fn task_ids_for_process(pid : ProcessId) -> Option<Vec<TaskId>> {
    active_impl::task_ids_for_process(pid)
}

/// 查询进程内任务语义快照。
#[inline]
pub fn process_task_snapshot(task_id : TaskId) -> Option<ProcessTaskDescriptor> {
    active_impl::lookup_task(task_id)
}

/// 按调度实体 `TaskId` 反查其进程归属快照。
#[inline]
pub fn process_task_snapshot_by_task(task_id : TaskId) -> Option<ProcessTaskDescriptor> {
    active_impl::lookup_task(task_id)
}

/// 当前运行任务对应的进程归属快照；未接入真实 spawn 前可能为 `None`。
#[inline]
pub fn current_process_task_snapshot() -> Option<ProcessTaskDescriptor> {
    let task_id = current_task_id()?;
    process_task_snapshot_by_task(task_id)
}

/// 当前运行任务所属进程快照。
#[inline]
pub fn current_process_snapshot() -> Option<ProcessDescriptor> {
    let pid = current_process_task_snapshot()?.pid;
    process_snapshot(pid)
}

/// 按进程号查找 leader task。
#[inline]
pub fn leader_task_for_process(pid : ProcessId) -> Option<TaskId> {
    active_impl::leader_task_for_process(pid)
}

/// 查找当前进程下一个已退出子进程。
#[inline]
pub fn find_exited_child_process(parent_pid : ProcessId) -> Option<ProcessDescriptor> {
    active_impl::find_exited_child_process(parent_pid)
}

/// 判断当前进程是否仍有子进程。
#[inline]
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
///
/// 每个 bring-up 脚本结束后调用，避免 fork 子进程泄漏页帧或破坏后续 spawn。
/// 返回 `(统计, 本次 reap 到的已退出任务)`，便于上层释放 cred / fd 等资源。
#[inline]
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
                if kill_task(task_id, -1) {
                    stats.killed_tasks = stats.killed_tasks.saturating_add(1);
                    progress = true;
                }
            }
        }
        for pid in active_impl::collect_exited_process_pids() {
            if let Some(exited) = reap_exited_process(pid) {
                stats.reaped_processes = stats.reaped_processes.saturating_add(1);
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
#[inline]
pub fn collect_exited_process_pids() -> Vec<ProcessId> {
    active_impl::collect_exited_process_pids()
}

/// 回收 registry 中所有已退出进程（含 basic 测试遗留、父进程已 reap 的僵尸子进程）。
#[inline]
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
#[inline]
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
#[inline]
pub fn set_task_clear_child_tid(task_id : TaskId, clear_child_tid : Option<TaskClearTid>) -> bool {
    active_impl::set_task_clear_child_tid(task_id, clear_child_tid)
}

#[inline]
pub fn task_clear_child_tid(task_id : TaskId) -> Option<TaskClearTid> {
    active_impl::task_clear_child_tid(task_id)
}

/// 进程 registry 自检。
#[inline]
pub fn process_model_self_test() {
    active_impl::process_model_self_test();
}

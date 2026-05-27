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

mod runtime;
pub mod wait_queue;
pub use self::wait_queue::WaitQueue;
mod scheduler {
    pub use scheduler::*;
}
pub use api_v0::{
    AddressSpaceHandle, KernelTaskEntry, TaskBlockReason, TaskExitCode, TaskSnapshot, TaskTick,
    TaskWaitResult, UserImageInfo, UserStack, UserTask, WaitQueueId,
};
pub use api_v0::{ExitedTask, TaskId, TaskWaitHandle};
#[cfg(feature = "impl-core")]
pub(crate) use impl_core as active_impl;
use mm_api::kernel_bringup::LoadedElf;

/// 初始化任务系统和底层调度器状态。
#[inline]
pub fn init() { scheduler::init(); }

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
pub fn spawn_user_task(user : UserTask) -> TaskId { scheduler::spawn_user_task(user) }

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
    scheduler::reap_exited_task(task_id)
}

/// 从当前用户任务 fork 一个子任务，并返回子任务 id。
///
/// 子任务获得父任务 trap 帧副本（a0 置 0），使用独立地址空间。
/// `child_stack` 非零时，子任务初始用户栈指针设为该值（用于 clone 新栈场景）。
/// `new_aspace_ptr` / `new_satp` 由 `mm::kernel_mm::fork_user_aspace()` 提供。
/// 无当前任务或当前不是用户任务时返回 `None`。
#[inline]
pub fn fork_current(child_stack : usize, new_aspace_ptr : usize, new_satp : usize) -> Option<TaskId> {
    scheduler::fork_current(child_stack, new_aspace_ptr, new_satp)
}

/// execve：替换当前任务的进程映像。
#[inline]
pub fn execve_current(entry_pc : usize,
                      sp : usize,
                      satp : usize,
                      user_aspace_ptr : usize,
                      image_info : UserImageInfo,
                      stack_info : UserStack) {
    scheduler::execve_current(entry_pc, sp, satp, user_aspace_ptr, image_info, stack_info)
}

/// 回收一个任意已退出任务的信息。
#[inline]
pub fn reap_one_exited_task() -> Option<ExitedTask> { scheduler::reap_one_exited_task() }

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
pub fn exit_current(exit_code : TaskExitCode) -> ! { scheduler::exit_current(exit_code) }

/// 终止指定任务（非当前任务）。
#[inline]
pub fn kill_task(task_id : TaskId, exit_code : TaskExitCode) -> bool {
    scheduler::kill_task(task_id, exit_code)
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

//! 调度器聚合包：把 **调度算法实现** 接到统一的进程内 API
//! 上，并转发给内核其余部分。
//!
//! ## 与 `impl-core` 的边界
//!
//! - **本 crate**：维护“当前任务”、就绪/阻塞结构、等待队列与唤醒路径，
//!   在合适时机调用 arch 的 `__switch` 等完成 **CPU 让渡与再入**；对外提供
//!   `init`/`spawn_*`/`schedule_tick`/trap 相关委托等 **调度语义**。
//! - **`wateros-task-impl-core`**：提供
//!   **单任务资源与现场**（TCB、内核栈、用户栈与 trap frame 的读写在 TCB
//!   内完成）；轮转等实现 **使用** 这些类型组装任务，但 **不负责** 定义
//!   `task_api` 中的公共 ID/等待抽象。
//!
//! 因此：改“下一个运行谁”主要动
//! `scheduler-impl`；改“任务结构体里存什么、栈多大”主要动 `impl-core`。
//!
//! 具体算法由 feature（如 `impl-round-robin`）选择，通过 `active_impl` 重导出。

#![no_std]

#[cfg(all(feature = "impl-multi-class", feature = "impl-round-robin"))]
compile_error!("features `impl-multi-class` and `impl-round-robin` are mutually exclusive");

#[cfg(not(any(feature = "impl-multi-class", feature = "impl-round-robin")))]
compile_error!("one scheduler implementation feature must be enabled");

pub mod api {
    pub use api_v0::*;
}

#[cfg(feature = "impl-multi-class")]
pub use impl_multi_class as active_impl;

#[cfg(feature = "impl-round-robin")]
pub use impl_round_robin as active_impl;

pub use api_v0::{ScheduleReason, SchedPolicyChangeAction, Scheduler, SwitchScheduler};
/// 当前架构下活动 trap 帧的具体类型别名；与 `wateros-task` 聚合层及
/// `impl-round-robin` 一致。
pub type TaskTrapFrame = arch::trap::ActiveTrapFrame;
pub use task_api::{
    AddressSpaceHandle, ExitedTask, KernelTaskEntry, TaskBlockReason, TaskExitCode, TaskId,
    TaskKind, TaskSnapshot, TaskState, TaskTick, TaskWaitHandle, TaskWaitResult, TaskWaitTarget,
    UserImageInfo, UserTask, UserTaskEntryPc, WaitQueueId, IDLE_TASK_ID,
};

/// 初始化当前启用的调度器实现。
#[inline]
pub fn init() { active_impl::init_scheduler(); }

/// 应用调度策略变更；由 [`wateros-task::sched`] 在 syscall 路径调用。
#[inline]
pub fn apply_sched_policy_change(task_id : TaskId,
                                 policy : task_api::SchedPolicy,
                                 param : task_api::SchedParam)
                                 -> Result<SchedPolicyChangeAction, task_api::SchedError>
{
    active_impl::apply_sched_policy_change(task_id, policy, param)
}

/// 当前运行任务的用户态地址空间 token（`0` 表示使用内核地址空间）。
#[inline]
pub fn current_task_address_space_raw() -> usize { active_impl::current_task_address_space_raw() }

/// 当前运行任务的 Sv39 用户页表对象指针；`0` 表示无。
#[inline]
pub fn current_task_user_aspace_ptr() -> usize { active_impl::current_task_user_aspace_ptr() }
pub fn current_task_user_address_space_token() -> usize {
    active_impl::current_task_user_address_space_token()
}
pub fn current_task_trap_return_address_space_token() -> usize {
    active_impl::current_task_trap_return_address_space_token()
}

/// 创建一个新的内核任务，并返回其任务号。
#[inline]
pub fn spawn_kernel_task(entry : KernelTaskEntry, arg : usize) -> TaskId {
    active_impl::spawn_kernel_task(entry, arg)
}

/// 按给定规格创建一个新的用户任务，并返回其任务号。
#[inline]
pub fn spawn_user_task(spec : UserTask) -> TaskId { active_impl::spawn_user_task_spec(spec) }
/// 为上层同步对象分配一个等待队列编号。
#[inline]
pub fn allocate_wait_queue() -> WaitQueueId { active_impl::allocate_wait_queue() }

/// 从当前用户任务 fork 一个子任务，并返回子任务 id。
///
/// 子任务获得父 trap 帧副本（a0 置 0）、独立地址空间（`new_aspace_ptr` /
/// `new_satp`）。 `child_stack` 非零时，子任务初始用户栈指针设为该值（用于
/// clone 新栈场景）。
#[inline]
pub fn fork_current(child_stack : usize,
                    new_aspace_ptr : usize,
                    new_satp : usize)
                    -> Option<TaskId> {
    active_impl::fork_current(child_stack, new_aspace_ptr, new_satp)
}

/// 从当前用户任务 clone 一个同进程线程。
#[inline]
pub fn clone_current_thread(child_stack : usize, tls : usize, set_tls : bool) -> Option<TaskId> {
    active_impl::clone_current_thread(child_stack, tls, set_tls)
}

/// execve：替换当前任务进程映像。
#[inline]
pub fn execve_current(entry_pc : usize,
                      sp : usize,
                      argc : usize,
                      argv : usize,
                      envp : usize,
                      satp : usize,
                      user_aspace_ptr : usize,
                      image_info : task_api::UserImageInfo,
                      stack_info : task_api::UserStack) {
    active_impl::execve_current(entry_pc,
                                sp,
                                argc,
                                argv,
                                envp,
                                satp,
                                user_aspace_ptr,
                                image_info,
                                stack_info)
}

/// 启动调度器并切入第一个可运行任务。
#[inline]
pub fn run_first_task() -> ! { active_impl::run_first_task() }

/// 挂起当前任务并调度下一个可运行任务。
#[inline]
pub fn suspend_current_and_run_next() { active_impl::suspend_current_and_run_next(); }

/// 处理一次时钟 tick 相关的调度逻辑。
#[inline]
pub fn schedule_tick() { active_impl::schedule_tick(); }

/// 按给定原因阻塞当前任务。
#[inline]
pub fn block_current(reason : TaskBlockReason) { active_impl::block_current(reason); }

/// 让当前任务等待指定的阻塞对象。
#[inline]
pub fn wait_current(wait_handle : TaskWaitHandle) { active_impl::wait_current(wait_handle); }

/// 在调度临界区内复查条件；条件为真才阻塞当前任务。
#[inline]
pub fn wait_current_while(wait_handle : TaskWaitHandle, condition : impl FnOnce() -> bool) {
    active_impl::wait_current_while(wait_handle, condition);
}

/// 让当前任务等待指定的阻塞对象，并带一个超时。
#[inline]
pub fn wait_current_timeout(wait_handle : TaskWaitHandle,
                            timeout_ticks : TaskTick)
                            -> TaskWaitResult {
    active_impl::wait_current_timeout(wait_handle, timeout_ticks)
}

/// 在调度临界区内复查条件；条件为真才执行带超时等待。
#[inline]
pub fn wait_current_timeout_while(wait_handle : TaskWaitHandle,
                                  timeout_ticks : TaskTick,
                                  condition : impl FnOnce() -> bool)
                                  -> TaskWaitResult {
    active_impl::wait_current_timeout_while(wait_handle, timeout_ticks, condition)
}

/// 让当前任务在指定等待队列上休眠，直到被唤醒。
#[inline]
pub fn wait_current_on(wait_queue_id : WaitQueueId) {
    wait_current(TaskWaitHandle::for_wait_queue(wait_queue_id));
}

/// 让当前任务在指定等待队列上等待，并附带超时时间。
#[inline]
pub fn wait_current_on_timeout(wait_queue_id : WaitQueueId,
                               timeout_ticks : TaskTick)
                               -> TaskWaitResult {
    wait_current_timeout(TaskWaitHandle::for_wait_queue(wait_queue_id),
                         timeout_ticks)
}

/// 让当前任务等待指定任务退出。
#[inline]
pub fn wait_for_task_exit(task_id : TaskId) {
    wait_current(TaskWaitHandle::for_task_exit(task_id));
}

/// 让当前任务等待指定任务退出，并附带超时时间。
#[inline]
pub fn wait_for_task_exit_timeout(task_id : TaskId, timeout_ticks : TaskTick) -> TaskWaitResult {
    wait_current_timeout(TaskWaitHandle::for_task_exit(task_id),
                         timeout_ticks)
}

/// 让当前任务睡眠指定数量的 tick。
#[inline]
pub fn sleep_current_for_ticks(ticks : TaskTick) { active_impl::sleep_current_for_ticks(ticks); }

/// 尝试唤醒指定任务。
#[inline]
pub fn wake_task(task_id : TaskId) -> bool { active_impl::wake_task(task_id) }

/// 终止指定任务（非当前任务）；当前任务应使用 [`exit_current`].
#[inline]
pub fn kill_task(task_id : TaskId, exit_code : TaskExitCode) -> bool {
    active_impl::kill_task(task_id, exit_code)
}

/// 回收指定已退出任务的退出信息。
#[inline]
pub fn reap_exited_task(task_id : TaskId) -> Option<ExitedTask> {
    active_impl::reap_exited_task(task_id)
}

/// 回收一个任意已退出任务的退出信息。
#[inline]
pub fn reap_one_exited_task() -> Option<ExitedTask> { active_impl::reap_one_exited_task() }

/// 回收指定父任务下一个已退出子任务的信息。
#[inline]
pub fn reap_one_exited_child(parent_id : TaskId) -> Option<ExitedTask> {
    active_impl::reap_one_exited_child(parent_id)
}

/// 判断指定任务是否仍有子任务。
#[inline]
pub fn has_child(parent_id : TaskId) -> bool { active_impl::has_child(parent_id) }

/// 从指定等待队列中唤醒一个任务。
#[inline]
pub fn wake_one_in_wait_queue(wait_queue_id : WaitQueueId) -> Option<TaskId> {
    active_impl::wake_one_in_wait_queue(wait_queue_id)
}

/// 唤醒指定等待队列中的全部任务，并返回唤醒数量。
#[inline]
pub fn wake_all_in_wait_queue(wait_queue_id : WaitQueueId) -> usize {
    active_impl::wake_all_in_wait_queue(wait_queue_id)
}

/// 让当前任务以给定退出码结束运行。
#[inline]
pub fn exit_current(exit_code : TaskExitCode) -> ! { active_impl::exit_current(exit_code) }

/// 返回当前正在运行任务的任务号。
#[inline]
pub fn current_task_id() -> Option<TaskId> { active_impl::current_task_id() }

/// 返回当前正在运行任务的稳定快照。
#[inline]
pub fn current_task_snapshot() -> Option<TaskSnapshot> { active_impl::current_task_snapshot() }

/// 返回指定任务的稳定快照；任务不存在或已被回收时返回 `None`。
#[inline]
pub fn task_snapshot(task_id : TaskId) -> Option<TaskSnapshot> {
    active_impl::task_snapshot(task_id)
}

/// 返回当前调度器逻辑 tick。
#[inline]
pub fn current_tick() -> TaskTick { active_impl::current_tick() }

/// 返回当前任务用于处理 trap 的内核栈顶地址。
#[inline]
pub fn current_task_kernel_stack_top() -> Option<usize> {
    active_impl::current_task_kernel_stack_top()
}

/// 将当前 trap 现场装载到当前任务对象，并返回权威 trap frame 指针。
#[inline]
pub fn begin_current_trap_frame_access(trap_frame : TaskTrapFrame) -> Option<*mut TaskTrapFrame> {
    active_impl::begin_current_trap_frame_access(trap_frame)
}

/// 将当前任务中保存的 trap 现场恢复到给定缓冲区。
#[inline]
pub fn restore_current_trap_frame(trap_frame : &mut TaskTrapFrame) -> bool {
    active_impl::restore_current_trap_frame(trap_frame)
}

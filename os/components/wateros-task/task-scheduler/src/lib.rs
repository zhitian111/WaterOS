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

extern crate alloc;

use alloc::vec::Vec;

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

pub use api_v0::{SchedPolicyChangeAction, ScheduleReason, Scheduler, SwitchScheduler};
/// 当前架构下活动 trap 帧的具体类型别名；与 `wateros-task` 聚合层及
/// `impl-round-robin` 一致。
pub type TaskTrapFrame = arch::trap::ActiveTrapFrame;
pub use task_api::{
    AddressSpaceHandle, ExitedTask, KernelTaskEntry, TaskExitCode, TaskId,
    TaskKind, TaskSnapshot, TaskState, TaskTick, TaskWaitResult, TaskWaitTarget,
    UserImageInfo, UserTask, UserTaskEntryPc, WaitQueueId, IDLE_TASK_ID,
};

/// 初始化当前启用的调度器实现。
pub fn init() {
    log::warn!("[boot-init] task_scheduler::init -> active_impl::init_scheduler");
    active_impl::init_scheduler();
    log::warn!("[boot-init] task_scheduler::init done");
}

/// 应用调度策略变更；由 [`wateros-task::sched`] 在 syscall 路径调用。
pub fn apply_sched_policy_change(task_id : TaskId,
                                 policy : task_api::SchedPolicy,
                                 param : task_api::SchedParam)
                                 -> Result<SchedPolicyChangeAction, task_api::SchedError> {
    active_impl::apply_sched_policy_change(task_id, policy, param)
}

/// 当前运行任务的用户态地址空间 token（`0` 表示使用内核地址空间）。
pub fn current_task_address_space_raw() -> usize { active_impl::current_task_address_space_raw() }

/// 当前运行任务的 Sv39 用户页表对象指针；`0` 表示无。
pub fn current_task_user_aspace_ptr() -> usize { active_impl::current_task_user_aspace_ptr() }
pub fn current_task_user_address_space_token() -> usize {
    active_impl::current_task_user_address_space_token()
}
pub fn current_task_trap_return_address_space_token() -> usize {
    active_impl::current_task_trap_return_address_space_token()
}

/// 创建一个新的内核任务，并返回其任务号。
pub fn spawn_kernel_task(entry : KernelTaskEntry, arg : usize) -> TaskId {
    active_impl::spawn_kernel_task(entry, arg)
}

/// 按给定规格创建一个新的用户任务，并返回其任务号。
pub fn spawn_user_task(spec : UserTask) -> TaskId { active_impl::spawn_user_task_spec(spec) }

/// 按给定规格创建用户任务（仅登记 TCB，不入就绪队列）。
pub fn create_user_task_spec(spec : UserTask) -> TaskId { active_impl::create_user_task_spec(spec) }

/// 将已创建任务加入就绪队列。
pub fn enqueue_ready_task(task_id : TaskId) { active_impl::enqueue_ready_task(task_id) }
/// 为上层同步对象分配一个等待队列编号。
pub fn allocate_wait_queue() -> WaitQueueId { active_impl::allocate_wait_queue() }

/// 当等待队列为空时释放编号供后续同步对象复用。
pub fn try_release_wait_queue(wait_queue_id : WaitQueueId) -> bool {
    active_impl::try_release_wait_queue(wait_queue_id)
}

/// 从当前用户任务 fork 一个子任务，并返回子任务 id。
pub fn fork_current(child_stack : usize,
                    new_aspace_ptr : usize,
                    new_satp : usize)
                    -> Option<TaskId> {
    active_impl::fork_current(child_stack, new_aspace_ptr, new_satp)
}

/// 从当前用户任务 fork 子任务（仅登记 TCB，不入就绪队列）。
pub fn create_fork_child(child_stack : usize,
                         new_aspace_ptr : usize,
                         new_satp : usize)
                         -> Option<TaskId> {
    active_impl::create_fork_child(child_stack, new_aspace_ptr, new_satp)
}

/// 从当前用户任务 clone 一个同进程线程。
pub fn clone_current_thread(child_stack : usize, tls : usize, set_tls : bool) -> Option<TaskId> {
    active_impl::clone_current_thread(child_stack, tls, set_tls)
}

/// 从当前用户任务 clone 线程（仅登记 TCB，不入就绪队列）。
pub fn create_clone_thread(child_stack : usize, tls : usize, set_tls : bool) -> Option<TaskId> {
    active_impl::create_clone_thread(child_stack, tls, set_tls)
}

/// 丢弃 fork/clone 失败时已登记但未应继续运行的子任务。
pub fn discard_unstarted_task(task_id : TaskId) { active_impl::discard_unstarted_task(task_id) }

/// execve：替换当前任务进程映像。
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
pub fn run_first_task() -> ! { active_impl::run_first_task() }

/// 挂起当前任务并调度下一个可运行任务。
pub fn suspend_current_and_run_next() { active_impl::suspend_current_and_run_next(); }

/// 处理一次时钟 tick 相关的调度逻辑。
pub fn schedule_tick() { active_impl::schedule_tick(); }

/// 按给定原因阻塞当前任务。
pub fn block_current(reason : TaskWaitTarget) { active_impl::block_current(reason); }

/// 让当前任务等待指定的阻塞对象。
pub fn wait_current(target : TaskWaitTarget) -> TaskWaitResult {
    active_impl::wait_current(target)
}

/// 在调度临界区内复查条件；条件为真才阻塞当前任务。
pub fn wait_current_while(target : TaskWaitTarget,
                          condition : impl FnOnce() -> bool)
                          -> TaskWaitResult {
    active_impl::wait_current_while(target, condition)
}

/// 让当前任务等待指定的阻塞对象，并带一个超时。
pub fn wait_current_timeout(target : TaskWaitTarget,
                            timeout_ticks : TaskTick)
                            -> TaskWaitResult {
    active_impl::wait_current_timeout(target, timeout_ticks)
}

/// 在调度临界区内复查条件；条件为真才执行带超时等待。
pub fn wait_current_timeout_while(target : TaskWaitTarget,
                                  timeout_ticks : TaskTick,
                                  condition : impl FnOnce() -> bool)
                                  -> TaskWaitResult {
    active_impl::wait_current_timeout_while(target, timeout_ticks, condition)
}

/// 让当前任务在指定等待队列上休眠，直到被唤醒。
pub fn wait_current_on(wait_queue_id : WaitQueueId) -> TaskWaitResult {
    wait_current(TaskWaitTarget::WaitQueue(wait_queue_id))
}

/// 让当前任务在指定等待队列上等待，并附带超时时间。
pub fn wait_current_on_timeout(wait_queue_id : WaitQueueId,
                               timeout_ticks : TaskTick)
                               -> TaskWaitResult {
    wait_current_timeout(TaskWaitTarget::WaitQueue(wait_queue_id),
                         timeout_ticks)
}

/// 让当前任务等待指定任务退出。
pub fn wait_for_task_exit(task_id : TaskId) -> TaskWaitResult {
    wait_current(TaskWaitTarget::TaskExit(task_id))
}

/// 让当前任务等待指定任务退出，并附带超时时间。
pub fn wait_for_task_exit_timeout(task_id : TaskId, timeout_ticks : TaskTick) -> TaskWaitResult {
    wait_current_timeout(TaskWaitTarget::TaskExit(task_id),
                         timeout_ticks)
}

/// 让当前任务睡眠指定数量的 tick。
pub fn sleep_current_for_ticks(ticks : TaskTick) -> TaskWaitResult {
    active_impl::sleep_current_for_ticks(ticks)
}

/// 尝试唤醒指定任务。
pub fn wake_task(task_id : TaskId) -> bool { active_impl::wake_task(task_id) }

pub fn interrupt_task(task_id : TaskId) -> bool { active_impl::interrupt_task(task_id) }

pub fn block_task_manual(task_id : TaskId) { active_impl::block_task_manual(task_id) }

pub fn wake_child_exit_waiters(parent_id : TaskId) {
    active_impl::wake_child_exit_waiters(parent_id)
}

/// 终止指定任务（非当前任务）；当前任务应使用 [`exit_current`].
pub fn kill_task(task_id : TaskId, exit_code : TaskExitCode) -> bool {
    active_impl::kill_task(task_id, exit_code)
}

/// 回收指定已退出任务的退出信息。
pub fn reap_exited_task(task_id : TaskId) -> Option<ExitedTask> {
    active_impl::reap_exited_task(task_id)
}

/// 在单次关中断临界区内批量回收已退出任务。
pub fn reap_exited_tasks_atomic(take_task_ids : impl FnOnce() -> Vec<TaskId>) -> Vec<ExitedTask> {
    active_impl::reap_exited_tasks_atomic(take_task_ids)
}

/// 回收一个任意已退出任务的退出信息。
pub fn reap_one_exited_task() -> Option<ExitedTask> { active_impl::reap_one_exited_task() }

/// 回收指定父任务下一个已退出子任务的信息。
pub fn reap_one_exited_child(parent_id : TaskId) -> Option<ExitedTask> {
    active_impl::reap_one_exited_child(parent_id)
}

/// 判断指定任务是否仍有子任务。
pub fn has_child(parent_id : TaskId) -> bool { active_impl::has_child(parent_id) }

/// 从指定等待队列中唤醒一个任务。
pub fn wake_one_in_wait_queue(wait_queue_id : WaitQueueId) -> Option<TaskId> {
    active_impl::wake_one_in_wait_queue(wait_queue_id)
}

/// 唤醒指定等待队列中的全部任务，并返回唤醒数量。
pub fn wake_all_in_wait_queue(wait_queue_id : WaitQueueId) -> usize {
    active_impl::wake_all_in_wait_queue(wait_queue_id)
}

/// 从一个显式等待队列唤醒部分任务，并把其余等待者迁移到另一个等待队列。
pub fn requeue_wait_queue(from_wait_queue_id : WaitQueueId,
                          to_wait_queue_id : WaitQueueId,
                          wake_count : usize,
                          requeue_count : usize)
                          -> usize {
    active_impl::requeue_wait_queue(from_wait_queue_id,
                                    to_wait_queue_id,
                                    wake_count,
                                    requeue_count)
}

/// 让当前任务以给定退出码结束运行。
pub fn exit_current(exit_code : TaskExitCode) -> ! { active_impl::exit_current(exit_code) }

/// 返回当前正在运行任务的任务号。
pub fn current_task_id() -> Option<TaskId> { active_impl::current_task_id() }

/// 返回当前正在运行任务的稳定快照。
pub fn current_task_snapshot() -> Option<TaskSnapshot> { active_impl::current_task_snapshot() }

/// 返回指定任务的稳定快照；任务不存在或已被回收时返回 `None`。
pub fn task_snapshot(task_id : TaskId) -> Option<TaskSnapshot> {
    active_impl::task_snapshot(task_id)
}

/// 返回当前调度器逻辑 tick。
pub fn current_tick() -> TaskTick { active_impl::current_tick() }

/// 返回当前任务用于处理 trap 的内核栈顶地址。
pub fn current_task_kernel_stack_top() -> Option<usize> {
    active_impl::current_task_kernel_stack_top()
}

/// 将当前 trap 现场装载到当前任务对象，并返回权威 trap frame 指针。
pub fn begin_current_trap_frame_access(trap_frame : TaskTrapFrame) -> Option<*mut TaskTrapFrame> {
    active_impl::begin_current_trap_frame_access(trap_frame)
}

/// 将当前任务中保存的 trap 现场恢复到给定缓冲区。
pub fn restore_current_trap_frame(trap_frame : &mut TaskTrapFrame) -> bool {
    active_impl::restore_current_trap_frame(trap_frame)
}

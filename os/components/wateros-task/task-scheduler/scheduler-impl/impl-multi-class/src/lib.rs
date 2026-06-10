//! 多类调度（OTHER+FIFO+RR） **具体实现**：就绪队列、等待队列注册表与一次调度决策，最终通过 arch
//! `__switch` 切换任务上下文。
//!
//! 任务体与 trap 现场由 `wateros-task-impl-core` 的 TCB 承载；本 crate 内
//! `scheduler` 子模块中的轮转状态 **引用并更新** 这些 TCB，但 **不** 替代
//! `impl-core` 对栈与 trap 缓冲区的所有权与初始化逻辑。

#![no_std]
#![allow(static_mut_refs)]

use arch::interrupt::ArchInterruptState;
use arch::task::ActiveArchTaskContext as TaskContext;
use base::sync::UniprocessorSafeCell;
use core::mem::MaybeUninit;
use core::sync::atomic::{AtomicBool, Ordering};
use task_api::{
    ExitedTask, KernelTaskEntry, TaskBlockReason, TaskExitCode, TaskId, TaskSnapshot, TaskTick,
    TaskWaitHandle, TaskWaitResult, UserTask, WaitQueueId,
};


mod queues;
mod rt_fifo_queue;
mod rt_rr_queue;
mod scheduler;
use api_v0::{SchedPolicyChangeAction, ScheduleReason};
use scheduler::MultiClassScheduler;
use task_api::{SchedError, SchedParam, SchedPolicy};

/// 与本实现 crate 中 `MultiClassScheduler` 使用的 trap
/// 帧类型一致，供聚合层类型别名复用。
pub type TaskTrapFrame = arch::trap::ActiveTrapFrame;

unsafe extern "C" {
    /// 架构提供的上下文切换：保存 `current`、恢复 `next`，约定与
    /// `ActiveArchTaskContext` 布局一致。
    fn __switch(current_task_cx_ptr : *mut TaskContext, next_task_cx_ptr : *const TaskContext);
}

pub type SwitchPair = api_v0::SwitchPair;

// 单处理器 bring-up：全局唯一调度器实例，由 `init_scheduler`
// 一次性写入；`SCHEDULER_READY` 保证可见性。
static mut SCHEDULER : MaybeUninit<UniprocessorSafeCell<MultiClassScheduler>> =
    MaybeUninit::uninit();
static SCHEDULER_READY : AtomicBool = AtomicBool::new(false);

// 仅在 `SCHEDULER_READY` 为真后解引用；否则 panic，避免未初始化访问。
fn scheduler_cell() -> &'static UniprocessorSafeCell<MultiClassScheduler> {
    assert!(SCHEDULER_READY.load(Ordering::Acquire),
            "scheduler not initialized: call init_scheduler() first");
    unsafe { &*SCHEDULER.as_ptr() }
}

// 在单调度器 cell 上取得独占引用并执行闭包；调用方已通过 `InterruptGuard`
// 关中断时保证不与其他 CPU 交错（当前为 UP 假设）。
fn with_scheduler<R>(f : impl FnOnce(&mut MultiClassScheduler) -> R) -> R {
    let mut scheduler = scheduler_cell().exclusive_access();
    f(&mut scheduler)
}

// RAII：构造时关全局中断，drop 时恢复；包裹所有可能触碰就绪队列与 TCB
// 的调度路径。
struct InterruptGuard {
    state : ArchInterruptState,
}

impl InterruptGuard {
    fn new() -> Self {
        let state = arch::interrupt::read_global_interrupt_state().expect("read global interrupt \
                                                                           state for scheduler \
                                                                           guard");
        arch::interrupt::disable_global_interrupt().expect("disable global interrupt for \
                                                            scheduler guard");
        Self { state }
    }

    /// 在即将 `__switch` 且 **不会** 再回到本栈帧（例如
    /// `exit_current`）时调用：立刻恢复关中断前状态， 并用 `forget` 避免
    /// `Drop` 二次恢复。否则下一条任务会永远继承「中断仍关闭」。
    fn release_before_switch(self) {
        let state = self.state;
        core::mem::forget(self);
        arch::interrupt::restore_global_interrupt_state(state).expect("restore global interrupt \
                                                                       state before context \
                                                                       switch");
    }
}

impl Drop for InterruptGuard {
    fn drop(&mut self) {
        arch::interrupt::restore_global_interrupt_state(self.state).expect("restore global \
                                                                            interrupt state for \
                                                                            scheduler guard");
    }
}

/// 返回当前运行任务的用户地址空间 token；`0` 表示回落到内核地址空间。
pub fn current_task_address_space_raw() -> usize {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.current_task_address_space_raw())
}

pub fn current_task_user_aspace_ptr() -> usize {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.current_task_user_aspace_ptr())
}

pub fn current_task_user_address_space_token() -> usize {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.current_task_user_address_space_token())
}

pub fn current_task_trap_return_address_space_token() -> usize {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.current_task_trap_return_address_space_token())
}

/// 应用调度策略变更（完整版：detach/入队/必要时 RescheduleNow）。
pub fn apply_sched_policy_change(task_id : TaskId,
                                 policy : SchedPolicy,
                                 param : SchedParam)
                                 -> Result<SchedPolicyChangeAction, SchedError> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.apply_sched_policy_change(task_id, policy, param))
}

/// 幂等初始化全局调度器与内部 `MultiClassScheduler` 状态。
pub fn init_scheduler() {
    if !SCHEDULER_READY.load(Ordering::Acquire) {
        unsafe {
            SCHEDULER.write(UniprocessorSafeCell::new(MultiClassScheduler::new()));
        }
        SCHEDULER_READY.store(true, Ordering::Release);
    }
    with_scheduler(|scheduler| scheduler.init());
    log::info!("[task-scheduler] initialized");
}

/// 创建内核任务并入就绪队列尾部。
pub fn spawn_kernel_task(entry : KernelTaskEntry, arg : usize) -> TaskId {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.spawn_kernel_task(entry, arg))
}

/// 按规格创建用户任务并入就绪队列尾部。
pub fn spawn_user_task_spec(spec : UserTask) -> TaskId {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.spawn_user_task_spec(spec))
}

/// 从当前用户任务 fork 一个子任务，并返回子任务 id。
///
/// 子任务获得父任务 trap 帧副本（a0 置 0）、独立地址空间（`new_aspace_ptr` /
/// `new_satp`）。 `child_stack` 非零时，子任务初始用户栈指针设为该值（用于
/// clone 新栈场景）。 无当前任务或非用户任务时返回 `None`。
pub fn fork_current(child_stack : usize,
                    new_aspace_ptr : usize,
                    new_satp : usize)
                    -> Option<TaskId> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.fork_current(child_stack, new_aspace_ptr, new_satp))
}

/// 从当前用户任务 clone 一个同进程线程；线程共享用户地址空间但有独立执行现场。
pub fn clone_current_thread(child_stack : usize, tls : usize, set_tls : bool) -> Option<TaskId> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.clone_current_thread(child_stack, tls, set_tls))
}

/// execve：替换当前任务的进程映像（地址空间、入口、栈）。
pub fn execve_current(entry_pc : usize,
                      sp : usize,
                      argc : usize,
                      argv : usize,
                      envp : usize,
                      satp : usize,
                      user_aspace_ptr : usize,
                      image_info : task_api::UserImageInfo,
                      stack_info : task_api::UserStack) {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| {
        scheduler.execve_current(entry_pc,
                                 sp,
                                 argc,
                                 argv,
                                 envp,
                                 satp,
                                 user_aspace_ptr,
                                 image_info,
                                 stack_info)
    });
}

/// 分配新的显式等待队列编号。
pub fn allocate_wait_queue() -> WaitQueueId {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.allocate_wait_queue())
}

/// 切入多任务运行：从引导上下文切换到第一个被选中的就绪任务（通常非 idle）。
pub fn run_first_task() -> ! {
    let (current_task_cx_ptr, next_task_cx_ptr) =
        with_scheduler(|scheduler| scheduler.prepare_first_switch());
    unsafe {
        __switch(current_task_cx_ptr, next_task_cx_ptr);
    }
    panic!("run_first_task must not return");
}

/// 当前任务重新入就绪队列尾部并切换到下一个任务（若无其他就绪任务则可能不切）。
pub fn suspend_current_and_run_next() {
    let _guard = InterruptGuard::new();
    let switch_pair = with_scheduler(|scheduler| scheduler.schedule(ScheduleReason::Yield));
    if let Some((current_task_cx_ptr, next_task_cx_ptr)) = switch_pair {
        unsafe {
            __switch(current_task_cx_ptr, next_task_cx_ptr);
        }
    }
}

/// 时钟 tick：推进调度器逻辑时间，并在需要时切换到下一任务。
pub fn schedule_tick() {
    let _guard = InterruptGuard::new();
    let switch_pair = with_scheduler(|scheduler| scheduler.schedule(ScheduleReason::Tick));
    if let Some((current_task_cx_ptr, next_task_cx_ptr)) = switch_pair {
        unsafe {
            __switch(current_task_cx_ptr, next_task_cx_ptr);
        }
    }
}

/// 以给定原因阻塞当前任务并切换出去。
pub fn block_current(reason : TaskBlockReason) {
    let _guard = InterruptGuard::new();
    let switch_pair = with_scheduler(|scheduler| scheduler.schedule(ScheduleReason::Block(reason)));
    if let Some((current_task_cx_ptr, next_task_cx_ptr)) = switch_pair {
        unsafe {
            __switch(current_task_cx_ptr, next_task_cx_ptr);
        }
    }
}

/// 无限期等待指定句柄；被唤醒后从切换点继续运行。
pub fn wait_current(wait_handle : TaskWaitHandle) {
    let _guard = InterruptGuard::new();
    let switch_pair = with_scheduler(|scheduler| scheduler.schedule_wait(wait_handle, None));
    if let Some((current_task_cx_ptr, next_task_cx_ptr)) = switch_pair {
        unsafe {
            __switch(current_task_cx_ptr, next_task_cx_ptr);
        }
    }
}

/// 在关中断调度临界区内复查条件；仅当条件仍成立时才把当前任务挂入等待队列。
pub fn wait_current_while(wait_handle : TaskWaitHandle, condition : impl FnOnce() -> bool) {
    let _guard = InterruptGuard::new();
    if !condition() {
        return;
    }
    let switch_pair = with_scheduler(|scheduler| scheduler.schedule_wait(wait_handle, None));
    if let Some((current_task_cx_ptr, next_task_cx_ptr)) = switch_pair {
        unsafe {
            __switch(current_task_cx_ptr, next_task_cx_ptr);
        }
    }
}

/// 带超时的等待；`timeout_ticks == 0` 时立即返回 [`TaskWaitResult::TimedOut`]
/// 且不切换。
pub fn wait_current_timeout(wait_handle : TaskWaitHandle,
                            timeout_ticks : TaskTick)
                            -> TaskWaitResult {
    if timeout_ticks == 0 {
        return TaskWaitResult::TimedOut;
    }

    let _guard = InterruptGuard::new();
    let switch_pair =
        with_scheduler(|scheduler| scheduler.schedule_wait(wait_handle, Some(timeout_ticks)));
    if let Some((current_task_cx_ptr, next_task_cx_ptr)) = switch_pair {
        unsafe {
            __switch(current_task_cx_ptr, next_task_cx_ptr);
        }
    }
    with_scheduler(|scheduler| scheduler.take_current_wait_result())
}

/// 带超时的条件等待；条件为假时立即按正常唤醒返回。
pub fn wait_current_timeout_while(wait_handle : TaskWaitHandle,
                                  timeout_ticks : TaskTick,
                                  condition : impl FnOnce() -> bool)
                                  -> TaskWaitResult {
    if timeout_ticks == 0 {
        return TaskWaitResult::TimedOut;
    }

    let _guard = InterruptGuard::new();
    if !condition() {
        return TaskWaitResult::Woken;
    }
    let switch_pair =
        with_scheduler(|scheduler| scheduler.schedule_wait(wait_handle, Some(timeout_ticks)));
    if let Some((current_task_cx_ptr, next_task_cx_ptr)) = switch_pair {
        unsafe {
            __switch(current_task_cx_ptr, next_task_cx_ptr);
        }
    }
    with_scheduler(|scheduler| scheduler.take_current_wait_result())
}

/// 在指定等待队列上无限期阻塞（语法糖）。
pub fn wait_current_on(wait_queue_id : WaitQueueId) {
    wait_current(TaskWaitHandle::for_wait_queue(wait_queue_id));
}

/// 在指定等待队列上带超时等待（语法糖）。
pub fn wait_current_on_timeout(wait_queue_id : WaitQueueId,
                               timeout_ticks : TaskTick)
                               -> TaskWaitResult {
    wait_current_timeout(TaskWaitHandle::for_wait_queue(wait_queue_id),
                         timeout_ticks)
}

/// 等待目标任务退出（语法糖）。
pub fn wait_for_task_exit(task_id : TaskId) {
    wait_current(TaskWaitHandle::for_task_exit(task_id));
}

/// 等待目标任务退出，带超时（语法糖）。
pub fn wait_for_task_exit_timeout(task_id : TaskId, timeout_ticks : TaskTick) -> TaskWaitResult {
    wait_current_timeout(TaskWaitHandle::for_task_exit(task_id),
                         timeout_ticks)
}

/// 睡眠至少 `ticks` 个调度 tick（实现中与 yield 类似地将 wake_tick 推后）。
pub fn sleep_current_for_ticks(ticks : TaskTick) {
    let _guard = InterruptGuard::new();
    let switch_pair = with_scheduler(|scheduler| scheduler.schedule(ScheduleReason::Sleep(ticks)));
    if let Some((current_task_cx_ptr, next_task_cx_ptr)) = switch_pair {
        unsafe {
            __switch(current_task_cx_ptr, next_task_cx_ptr);
        }
    }
}

/// 若任务处于可唤醒队列则移回就绪队列并返回 `true`。
pub fn wake_task(task_id : TaskId) -> bool {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.wake_task(task_id))
}

/// 终止指定任务（非当前任务）；成功返回 `true`，任务不存在或 idle 返回 `false`。
pub fn kill_task(task_id : TaskId, exit_code : TaskExitCode) -> bool {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.kill_task(task_id, exit_code))
}

/// 从已退出队列中按任务号回收退出信息。
pub fn reap_exited_task(task_id : TaskId) -> Option<ExitedTask> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.reap_exited_task(task_id))
}

/// 按 FIFO 从已退出队列回收一个任务的退出信息。
pub fn reap_one_exited_task() -> Option<ExitedTask> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.reap_one_exited_task())
}

/// 按 FIFO 近似顺序回收当前父任务下任意已退出子任务。
pub fn reap_one_exited_child(parent_id : TaskId) -> Option<ExitedTask> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.reap_one_exited_child(parent_id))
}

/// 判断指定任务是否仍有子任务。
pub fn has_child(parent_id : TaskId) -> bool {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.has_child(parent_id))
}

/// 从显式等待队列头部唤醒一个任务。
pub fn wake_one_in_wait_queue(wait_queue_id : WaitQueueId) -> Option<TaskId> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.wake_one_in_wait_queue(wait_queue_id))
}

/// 清空指定显式等待队列并将其中任务全部置为就绪。
pub fn wake_all_in_wait_queue(wait_queue_id : WaitQueueId) -> usize {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.wake_all_in_wait_queue(wait_queue_id))
}

/// 标记当前任务退出并切换到其他任务；不应返回到已退出任务。
pub fn exit_current(exit_code : TaskExitCode) -> ! {
    let guard = InterruptGuard::new();
    let switch_pair =
        with_scheduler(|scheduler| scheduler.schedule(ScheduleReason::Exit(exit_code)));
    if let Some((current_task_cx_ptr, next_task_cx_ptr)) = switch_pair {
        guard.release_before_switch();
        unsafe {
            __switch(current_task_cx_ptr, next_task_cx_ptr);
        }
        // `__switch` 不回到本帧；仅为满足 `-> !` 类型检查。
        unsafe {
            core::hint::unreachable_unchecked();
        }
    }
    guard.release_before_switch();
    panic!("exit_current must not resume the exited task");
}

/// 当前运行任务号；引导阶段尚未切换时为 `None`。
pub fn current_task_id() -> Option<TaskId> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.current_task_id())
}

/// 当前运行任务的稳定快照（语义层，不含内核栈指针等实现细节）。
pub fn current_task_snapshot() -> Option<TaskSnapshot> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.current_task_snapshot())
}

/// 指定任务的稳定快照；任务不存在或已被回收时返回 `None`。
pub fn task_snapshot(task_id : TaskId) -> Option<TaskSnapshot> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.task_snapshot(task_id))
}

/// 当前调度器逻辑 tick。
pub fn current_tick() -> TaskTick {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.current_tick())
}

/// 当前任务内核栈顶，供 trap/用户态恢复路径使用。
pub fn current_task_kernel_stack_top() -> Option<usize> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.current_task_kernel_stack_top())
}

/// 开始由 Rust 修改当前任务的权威 trap 上下文，返回可写指针（若尚无当前任务则为
/// `None`）。
pub fn begin_current_trap_frame_access(trap_frame : TaskTrapFrame) -> Option<*mut TaskTrapFrame> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.begin_current_trap_frame_access(trap_frame))
}

/// 将 TCB 中保存的 trap 现场恢复到调用方缓冲区。
pub fn restore_current_trap_frame(trap_frame : &mut TaskTrapFrame) -> bool {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.restore_current_trap_frame(trap_frame))
}

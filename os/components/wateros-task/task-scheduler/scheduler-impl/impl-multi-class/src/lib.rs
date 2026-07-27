#![no_std]
#![allow(static_mut_refs)]

extern crate alloc;
use alloc::vec::Vec;
use arch::cpu::{self, current_cpu_id};
use arch::interrupt::ArchInterruptState;
use arch::task::ActiveArchTaskContext as TaskContext;
use base::cpu::CpuMask;
use base::sync::MultiprocessorSafeCell;
use core::mem::MaybeUninit;
use core::panic::Location;
use core::sync::atomic::{compiler_fence, AtomicBool, Ordering};
use task_api::{
    CpuId, ExitedTask, KernelTaskEntry, Priority, TaskExitCode, TaskId, TaskSnapshot, TaskTick,
    TaskWaitResult, TaskWaitTarget, UserTask, WaitQueueId,
};

mod scheduler;
pub use api_v0::{CpuSnapshot, ScheduleReason};
use scheduler::MultiClassScheduler;
use task_api::{SchedError, SchedPolicy};

/// 与本实现 crate 中 `MultiClassScheduler` 使用的 trap
/// 帧类型一致，供聚合层类型别名复用。
pub type TaskTrapFrame = arch::trap::ActiveTrapFrame;

unsafe extern "C" {
    fn __switch(current_task_cx_ptr : *mut TaskContext, next_task_cx_ptr : *const TaskContext);
}
pub type SwitchPair = api_v0::SwitchPair;

// ── 内部静态 ──────────────────────────────────────────────────────
#[unsafe(link_section = ".bss.scheduler")]
static mut SCHEDULER : MaybeUninit<MultiprocessorSafeCell<MultiClassScheduler>> =
    MaybeUninit::uninit();
#[unsafe(link_section = ".bss.scheduler")]
static SCHEDULER_READY : AtomicBool = AtomicBool::new(false);
// ── scheduler cell 访问 ────────────────────────────────────────────
#[inline(never)]
fn scheduler_cell_inner(caller : &'static Location)
                        -> &'static MultiprocessorSafeCell<MultiClassScheduler> {
    if !SCHEDULER_READY.load(Ordering::Acquire) {
        panic!("scheduler not initialized: call init_scheduler() first, caller={}:{}",
               caller.file(),
               caller.line());
    }
    unsafe { &*SCHEDULER.as_ptr() }
}
/// 取得调度器 cell
#[track_caller]
fn scheduler_cell() -> &'static MultiprocessorSafeCell<MultiClassScheduler> {
    scheduler_cell_inner(Location::caller())
}
// 在单调度器 cell 上取得独占引用并执行闭包；调用方已通过 `InterruptGuard`
#[inline(never)]
fn with_scheduler<R>(f : impl FnOnce(&mut MultiClassScheduler) -> R) -> R {
    let mut scheduler = scheduler_cell().exclusive_access();
    f(&mut scheduler)
}

// ── 跨核 IPI 通知 ────────────────────────────────────────────────
fn dispatch_reschedules(targets : CpuMask, current_cpu_id : CpuId) {
    let mut remote = targets;
    let local_requested = remote.contains(current_cpu_id);
    remote.remove(current_cpu_id);
    if !remote.is_empty() {
        if let Err(error) = platform::smp::send_ipi(remote, platform::smp::IpiKind::Reschedule) {
            log::warn!("[ipi] directed reschedule notification failed: {:?}",
                       error);
        }
    }
    if local_requested {
        schedule_reschedule();
    }
}
// ── 中断守卫 ──────────────────────────────────────────────────────
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
    fn release(self) {
        let state = self.state;
        core::mem::forget(self);
        arch::interrupt::restore_global_interrupt_state(state).expect("restore global interrupt \
                                                                       state before context \
                                                                       switch");
    }
}
/// 唯一的 Rust 上下文切换出口。
#[inline(never)]
fn switch_and_unlock(guard : InterruptGuard, switch_pair : SwitchPair) {
    // `schedule` 已在锁内把 CPU 的 current-task cache 更新为 next。
    // 若此时先开中断，SSIP/Timer 可在真正 `__switch` 前打断旧任务：trap
    // 帧仍属于旧用户任务，scheduler 却会把它当成 next（常为 idle），造成
    // “返回用户态但 current 是非用户任务”的状态错配。
    //
    // 保持中断关闭直至寄存器/栈都切换完成。首次进入的任务运行时会自行开
    // 中断；已运行过的任务恢复到此函数时，再恢复它保存的原中断状态。
    unsafe {
        __switch(switch_pair.0, switch_pair.1);
    }
    guard.release();
}
/// `__switch` 返回后重新关中断，再取等待结果（避免 wait 路径长期关中断）。
fn finish_wait_after_switch(guard : InterruptGuard,
                            switch_pair : Option<SwitchPair>)
                            -> TaskWaitResult {
    if let Some(switch_pair) = switch_pair {
        switch_and_unlock(guard, switch_pair);
    } else {
        guard.release();
    }
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.take_current_wait_result(cpu::current_cpu_id()))
}
// =============================================================================
//  1. 初始化与引导
// =============================================================================

#[inline(never)]
pub fn init() {
    if !SCHEDULER_READY.load(Ordering::Acquire) {
        unsafe {
            SCHEDULER.write(MultiprocessorSafeCell::new(MultiClassScheduler::new()));
            (*SCHEDULER.as_mut_ptr()).exclusive_access()
                                     .init(current_cpu_id());
        }
        SCHEDULER_READY.store(true, Ordering::Release);
    } else {
        let _guard = InterruptGuard::new();
        with_scheduler(|scheduler| scheduler.init(current_cpu_id()));
    }
    log::info!("[task-scheduler] initialized");
}

/// 从引导上下文切换到第一个被选中的就绪任务
pub fn run_first_task() -> ! {
    let guard = InterruptGuard::new();
    let switch_pair = with_scheduler(|scheduler| scheduler.prepare_first_switch(current_cpu_id()));
    switch_and_unlock(guard, switch_pair);
    panic!("run_first_task_on_current_cpu must not return");
}

// =============================================================================
//  2. 任务创建（spawn / fork / clone / exec）
// =============================================================================

/// 创建内核任务并入就绪队列尾部。
#[inline(never)]
pub fn spawn_kernel_task(entry : KernelTaskEntry, arg : usize) -> TaskId {
    let cpu_id = cpu::current_cpu_id();
    let (task_id, targets) = {
        let _guard = InterruptGuard::new();
        with_scheduler(|scheduler| {
            let task_id = scheduler.spawn_kernel_task(entry, arg, cpu_id);
            let targets = scheduler.take_pending_reschedule_cpus();
            (task_id, targets)
        })
    };
    dispatch_reschedules(targets, cpu_id);
    task_id
}

/// 按规格创建用户任务（仅登记 TCB，不入就绪队列）。
pub fn create_user_task_spec(spec : UserTask) -> TaskId {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.create_user_task_spec(spec, cpu::current_cpu_id()))
}

/// 按规格创建用户任务并入就绪队列尾部。
pub fn spawn_user_task_spec(spec : UserTask) -> TaskId {
    let cpu_id = cpu::current_cpu_id();
    let (task_id, targets) = {
        let _guard = InterruptGuard::new();
        with_scheduler(|scheduler| {
            let task_id = scheduler.spawn_user_task_spec(spec, cpu_id);
            let targets = scheduler.take_pending_reschedule_cpus();
            (task_id, targets)
        })
    };
    dispatch_reschedules(targets, cpu_id);
    task_id
}

/// 将已创建任务加入就绪队列尾部。
pub fn enqueue_ready_task(task_id : TaskId) {
    let cpu_id = cpu::current_cpu_id();
    let targets = {
        let _guard = InterruptGuard::new();
        with_scheduler(|scheduler| {
            scheduler.enqueue_ready_task(task_id);
            scheduler.take_pending_reschedule_cpus()
        })
    };
    dispatch_reschedules(targets, cpu_id);
}

/// 从当前用户任务 fork 子任务（仅登记 TCB，不入就绪队列）。
pub fn create_fork_child(child_stack : usize,
                         new_aspace_ptr : usize,
                         new_satp : usize)
                         -> Option<TaskId> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| {
        scheduler.create_fork_child(child_stack,
                                    new_aspace_ptr,
                                    new_satp,
                                    cpu::current_cpu_id())
    })
}

/// 从当前用户任务 clone 线程（仅登记 TCB，不入就绪队列）。
pub fn create_clone_thread(child_stack : usize, tls : usize, set_tls : bool) -> Option<TaskId> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| {
        scheduler.create_clone_thread(child_stack,
                                      tls,
                                      set_tls,
                                      cpu::current_cpu_id())
    })
}

/// 丢弃 fork/clone 失败时已登记但未应继续运行的子任务。
pub fn discard_unstarted_task(task_id : TaskId) {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.discard_unstarted_task(task_id));
}

/// 从当前用户任务 fork 一个子任务，并返回子任务 id。
pub fn fork_current(child_stack : usize,
                    new_aspace_ptr : usize,
                    new_satp : usize)
                    -> Option<TaskId> {
    let cpu_id = cpu::current_cpu_id();
    let (child_id, targets) = {
        let _guard = InterruptGuard::new();
        with_scheduler(|scheduler| {
            let child_id = scheduler.fork_current(child_stack,
                                                  new_aspace_ptr,
                                                  new_satp,
                                                  cpu_id);
            let targets = scheduler.take_pending_reschedule_cpus();
            (child_id, targets)
        })
    };
    dispatch_reschedules(targets, cpu_id);
    child_id
}

/// 从当前用户任务 clone 一个同进程线程；线程共享用户地址空间但有独立执行现场。
pub fn clone_current_thread(child_stack : usize, tls : usize, set_tls : bool) -> Option<TaskId> {
    let cpu_id = cpu::current_cpu_id();
    let (child_id, targets) = {
        let _guard = InterruptGuard::new();
        with_scheduler(|scheduler| {
            let child_id = scheduler.clone_current_thread(child_stack, tls, set_tls, cpu_id);
            let targets = scheduler.take_pending_reschedule_cpus();
            (child_id, targets)
        })
    };
    dispatch_reschedules(targets, cpu_id);
    child_id
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
                                 stack_info,
                                 cpu::current_cpu_id())
    });
}

// =============================================================================
//  3. 调度入口（yield / tick / block / sleep / exit）
// =============================================================================

/// 当前任务重新入就绪队列尾部并切换到下一个任务（若无其他就绪任务则可能不切）。
pub fn suspend_current_and_run_next() {
    let _guard = InterruptGuard::new();
    let cpu_id = cpu::current_cpu_id();
    let (switch_pair, targets) = with_scheduler(|scheduler| {
        let switch_pair = scheduler.schedule(ScheduleReason::Yield, cpu_id);
        let mut targets = scheduler.take_pending_reschedule_cpus();
        if targets.contains(cpu_id) {
            targets.remove(cpu_id);
            assert!(scheduler.take_need_resched(cpu_id));
        }
        (switch_pair, targets)
    });
    dispatch_reschedules(targets, cpu_id);
    if let Some(switch_pair) = switch_pair {
        switch_and_unlock(_guard, switch_pair);
    }
}

/// 时钟 tick：推进调度器逻辑时间，并在需要时切换到下一任务。可能会唤醒其他 CPU 上睡眠的任务
#[inline(never)]
pub fn schedule_tick() {
    let guard = InterruptGuard::new();
    let cpu_id = cpu::current_cpu_id();
    let (switch_pair, targets) = with_scheduler(|scheduler| {
        let switch_pair = scheduler.schedule(ScheduleReason::Tick, cpu_id);
        let mut targets = scheduler.take_pending_reschedule_cpus();
        if targets.contains(cpu_id) {
            targets.remove(cpu_id);
            assert!(scheduler.take_need_resched(cpu_id));
        }
        (switch_pair, targets)
    });
    dispatch_reschedules(targets, cpu_id);
    if let Some(switch_pair) = switch_pair {
        switch_and_unlock(guard, switch_pair);
    }
}

/// 中断当前任务并切换到下一任务；若当前任务已被阻塞或退出则不切换。
pub fn schedule_reschedule() {
    let guard = InterruptGuard::new();
    let cpu_id = cpu::current_cpu_id();
    let (switch_pair, targets) = with_scheduler(|scheduler| {
        // boot code still executes on the firmware/early-kernel stack while
        // CPUState is only logically seeded with its idle task.  A local IPI
        // request caused by spawn must stay pending until run_first_task has
        // performed the real boot-context switch.
        if scheduler.boot_context_active(cpu_id) {
            return (None, CpuMask::EMPTY);
        }
        let switch_pair = if scheduler.take_need_resched(cpu_id) {
            scheduler.schedule(ScheduleReason::Reschedule, cpu_id)
        } else {
            None
        };
        let mut targets = scheduler.take_pending_reschedule_cpus();
        if targets.contains(cpu_id) {
            targets.remove(cpu_id);
            assert!(scheduler.take_need_resched(cpu_id));
        }
        (switch_pair, targets)
    });
    dispatch_reschedules(targets, cpu_id);
    if let Some(switch_pair) = switch_pair {
        switch_and_unlock(guard, switch_pair);
    }
}

/// 以给定原因阻塞当前任务并切换出去。
pub fn block_current(reason : TaskWaitTarget) {
    let guard = InterruptGuard::new();
    let cpu_id = cpu::current_cpu_id();
    let (switch_pair, targets) = with_scheduler(|scheduler| {
        let switch_pair = scheduler.schedule(ScheduleReason::Block(reason), cpu_id);
        let mut targets = scheduler.take_pending_reschedule_cpus();
        if targets.contains(cpu_id) {
            targets.remove(cpu_id);
            assert!(scheduler.take_need_resched(cpu_id));
        }
        (switch_pair, targets)
    });
    dispatch_reschedules(targets, cpu_id);
    if let Some(switch_pair) = switch_pair {
        switch_and_unlock(guard, switch_pair);
    }
}

/// 睡眠至少 `ticks` 个调度 tick（实现中与 yield 类似地将 wake_tick 推后）。
pub fn sleep_current_for_ticks(ticks : TaskTick) -> TaskWaitResult {
    let guard = InterruptGuard::new();
    let cpu_id = cpu::current_cpu_id();
    let (switch_pair, targets) = with_scheduler(|scheduler| {
        let switch_pair = scheduler.schedule(ScheduleReason::Sleep(ticks), cpu_id);
        let mut targets = scheduler.take_pending_reschedule_cpus();
        if targets.contains(cpu_id) {
            targets.remove(cpu_id);
            assert!(scheduler.take_need_resched(cpu_id));
        }
        (switch_pair, targets)
    });
    dispatch_reschedules(targets, cpu_id);
    finish_wait_after_switch(guard, switch_pair)
}

/// 标记当前任务退出并切换到其他任务；不应返回到已退出任务。
pub fn exit_current(exit_code : TaskExitCode) -> ! {
    let guard = InterruptGuard::new();
    let cpu_id = cpu::current_cpu_id();
    let (switch_pair, targets) = with_scheduler(|scheduler| {
        let switch_pair = scheduler.schedule(ScheduleReason::Exit(exit_code), cpu_id);
        let mut targets = scheduler.take_pending_reschedule_cpus();
        if targets.contains(cpu_id) {
            targets.remove(cpu_id);
            assert!(scheduler.take_need_resched(cpu_id));
        }
        (switch_pair, targets)
    });
    dispatch_reschedules(targets, cpu_id);
    match switch_pair {
        Some(switch_pair) => {
            switch_and_unlock(guard, switch_pair);
            // `__switch` 不应回到已退出任务的栈帧；仅为满足 `-> !` 类型检查。
            unsafe {
                core::hint::unreachable_unchecked();
            }
        }
        None => {
            drop(guard);
            panic!("exit_current must not resume the exited task");
        }
    }
}

// =============================================================================
//  4. 等待与唤醒
// =============================================================================

/// 等待指定等待目标；被唤醒后从切换点继续运行。
pub fn wait_current(target : TaskWaitTarget) -> TaskWaitResult {
    let guard = InterruptGuard::new();
    let cpu_id = cpu::current_cpu_id();
    let (switch_pair, targets) = with_scheduler(|scheduler| {
        let switch_pair = scheduler.schedule_wait(target, None, cpu_id);
        let mut targets = scheduler.take_pending_reschedule_cpus();
        if targets.contains(cpu_id) {
            targets.remove(cpu_id);
            assert!(scheduler.take_need_resched(cpu_id));
        }
        (switch_pair, targets)
    });
    dispatch_reschedules(targets, cpu_id);
    finish_wait_after_switch(guard, switch_pair)
}

/// 在关中断调度临界区内复查条件；仅当条件仍成立时才把当前任务挂入等待。
pub fn wait_current_while(target : TaskWaitTarget,
                          condition : impl FnOnce() -> bool)
                          -> TaskWaitResult {
    let guard = InterruptGuard::new();
    if !condition() {
        return TaskWaitResult::Woken;
    }
    let cpu_id = cpu::current_cpu_id();
    let (switch_pair, targets) = with_scheduler(|scheduler| {
        let switch_pair = scheduler.schedule_wait(target, None, cpu_id);
        let mut targets = scheduler.take_pending_reschedule_cpus();
        if targets.contains(cpu_id) {
            targets.remove(cpu_id);
            assert!(scheduler.take_need_resched(cpu_id));
        }
        (switch_pair, targets)
    });
    dispatch_reschedules(targets, cpu_id);
    finish_wait_after_switch(guard, switch_pair)
}

/// 带超时的等待；`timeout_ticks == 0` 时立即返回 [`TaskWaitResult::TimedOut`]
/// 且不切换。
pub fn wait_current_timeout(target : TaskWaitTarget, timeout_ticks : TaskTick) -> TaskWaitResult {
    if timeout_ticks == 0 {
        return TaskWaitResult::TimedOut;
    }

    let guard = InterruptGuard::new();
    let cpu_id = cpu::current_cpu_id();
    let (switch_pair, targets) = with_scheduler(|scheduler| {
        let switch_pair = scheduler.schedule_wait(target, Some(timeout_ticks), cpu_id);
        let mut targets = scheduler.take_pending_reschedule_cpus();
        if targets.contains(cpu_id) {
            targets.remove(cpu_id);
            assert!(scheduler.take_need_resched(cpu_id));
        }
        (switch_pair, targets)
    });
    dispatch_reschedules(targets, cpu_id);
    finish_wait_after_switch(guard, switch_pair)
}

/// 带超时的条件等待；条件为假时立即按正常唤醒返回。
pub fn wait_current_timeout_while(target : TaskWaitTarget,
                                  timeout_ticks : TaskTick,
                                  condition : impl FnOnce() -> bool)
                                  -> TaskWaitResult {
    if timeout_ticks == 0 {
        return TaskWaitResult::TimedOut;
    }

    let guard = InterruptGuard::new();
    if !condition() {
        return TaskWaitResult::Woken;
    }
    let cpu_id = cpu::current_cpu_id();
    let (switch_pair, targets) = with_scheduler(|scheduler| {
        let switch_pair = scheduler.schedule_wait(target, Some(timeout_ticks), cpu_id);
        let mut targets = scheduler.take_pending_reschedule_cpus();
        if targets.contains(cpu_id) {
            targets.remove(cpu_id);
            assert!(scheduler.take_need_resched(cpu_id));
        }
        (switch_pair, targets)
    });
    dispatch_reschedules(targets, cpu_id);
    finish_wait_after_switch(guard, switch_pair)
}

/// 在指定等待队列上无限期阻塞（语法糖）。
pub fn wait_current_on(wait_queue_id : WaitQueueId) -> TaskWaitResult {
    wait_current(TaskWaitTarget::WaitQueue(wait_queue_id))
}

/// 在指定等待队列上带超时等待（语法糖）。
pub fn wait_current_on_timeout(wait_queue_id : WaitQueueId,
                               timeout_ticks : TaskTick)
                               -> TaskWaitResult {
    wait_current_timeout(TaskWaitTarget::WaitQueue(wait_queue_id),
                         timeout_ticks)
}

/// 等待目标任务退出（语法糖）。
pub fn wait_for_task_exit(task_id : TaskId) -> TaskWaitResult {
    wait_current(TaskWaitTarget::TaskExit(task_id))
}

/// 等待目标任务退出，带超时（语法糖）。
pub fn wait_for_task_exit_timeout(task_id : TaskId, timeout_ticks : TaskTick) -> TaskWaitResult {
    wait_current_timeout(TaskWaitTarget::TaskExit(task_id),
                         timeout_ticks)
}

/// 若任务处于可唤醒队列则移回就绪队列并返回 `true`。
pub fn wake_task(task_id : TaskId) -> bool {
    let cpu_id = cpu::current_cpu_id();
    let (woken, targets) = {
        let _guard = InterruptGuard::new();
        with_scheduler(|scheduler| {
            let woken = scheduler.wake_task(task_id);
            let targets = scheduler.take_pending_reschedule_cpus();
            (woken, targets)
        })
    };
    dispatch_reschedules(targets, cpu_id);
    woken
}

pub fn interrupt_task(task_id : TaskId) -> bool {
    let cpu_id = cpu::current_cpu_id();
    let (interrupted, targets) = {
        let _guard = InterruptGuard::new();
        with_scheduler(|scheduler| {
            let interrupted = scheduler.interrupt_task(task_id);
            let targets = scheduler.take_pending_reschedule_cpus();
            (interrupted, targets)
        })
    };
    dispatch_reschedules(targets, cpu_id);
    interrupted
}

pub fn block_task_manual(task_id : TaskId) {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.block_task_manual(task_id, cpu::current_cpu_id()));
}

pub fn wake_child_exit_waiters(parent_id : TaskId) {
    let cpu_id = cpu::current_cpu_id();
    let targets = {
        let _guard = InterruptGuard::new();
        with_scheduler(|scheduler| {
            scheduler.wake_child_exit_waiters(parent_id);
            scheduler.take_pending_reschedule_cpus()
        })
    };
    dispatch_reschedules(targets, cpu_id);
}

// =============================================================================
//  5. 等待队列管理
// =============================================================================

/// 分配新的显式等待队列编号。
pub fn allocate_wait_queue() -> WaitQueueId {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.allocate_wait_queue())
}

/// 当显式等待队列为空时释放其编号。
pub fn try_release_wait_queue(wait_queue_id : WaitQueueId) -> bool {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.try_release_wait_queue(wait_queue_id))
}

/// 从显式等待队列头部唤醒一个任务。
pub fn wake_one_in_wait_queue(wait_queue_id : WaitQueueId) -> Option<TaskId> {
    let cpu_id = cpu::current_cpu_id();
    let (woken, targets) = {
        let _guard = InterruptGuard::new();
        with_scheduler(|scheduler| {
            let woken = scheduler.wake_one_in_wait_queue(wait_queue_id);
            let targets = scheduler.take_pending_reschedule_cpus();
            (woken, targets)
        })
    };
    dispatch_reschedules(targets, cpu_id);
    woken
}

/// 清空指定显式等待队列并将其中任务全部置为就绪。
pub fn wake_all_in_wait_queue(wait_queue_id : WaitQueueId) -> usize {
    let cpu_id = cpu::current_cpu_id();
    let (count, targets) = {
        let _guard = InterruptGuard::new();
        with_scheduler(|scheduler| {
            let count = scheduler.wake_all_in_wait_queue(wait_queue_id);
            let targets = scheduler.take_pending_reschedule_cpus();
            (count, targets)
        })
    };
    dispatch_reschedules(targets, cpu_id);
    count
}

/// 从一个显式等待队列唤醒部分任务，并把其余等待者迁移到另一个等待队列。
pub fn requeue_wait_queue(from_wait_queue_id : WaitQueueId,
                          to_wait_queue_id : WaitQueueId,
                          wake_count : usize,
                          requeue_count : usize)
                          -> usize {
    let cpu_id = cpu::current_cpu_id();
    let (changed, targets) = {
        let _guard = InterruptGuard::new();
        with_scheduler(|scheduler| {
            let changed = scheduler.requeue_wait_queue(from_wait_queue_id,
                                                       to_wait_queue_id,
                                                       wake_count,
                                                       requeue_count);
            let targets = scheduler.take_pending_reschedule_cpus();
            (changed, targets)
        })
    };
    dispatch_reschedules(targets, cpu_id);
    changed
}

// =============================================================================
//  6. 退出回收
// =============================================================================

/// 在单次关中断临界区内批量回收已退出任务（避免 take 与 reap 之间插入调度）。
pub fn reap_exited_tasks_atomic(take_task_ids : impl FnOnce() -> Vec<TaskId>) -> Vec<ExitedTask> {
    let _guard = InterruptGuard::new();
    let task_ids = take_task_ids();
    with_scheduler(|scheduler| {
        let mut reaped = Vec::new();
        for task_id in task_ids {
            if let Some(exited) = scheduler.reap_exited_task(task_id) {
                reaped.push(exited);
            }
        }
        reaped
    })
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

/// 终止指定任务（非当前任务）；成功返回 `true`，任务不存在或 idle 返回 `false`。
pub fn kill_task(task_id : TaskId, exit_code : TaskExitCode) -> bool {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.kill_task(task_id, exit_code))
}

// =============================================================================
//  7. 查询接口
// =============================================================================

/// 当前运行任务号；引导阶段尚未切换时为 `None`。
pub fn current_task_id() -> Option<TaskId> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.current_task_id(cpu::current_cpu_id()))
}

/// 当前运行任务的稳定快照（语义层，不含内核栈指针等实现细节）。
pub fn current_task_snapshot() -> Option<TaskSnapshot> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.current_task_snapshot(cpu::current_cpu_id()))
}

/// 指定任务的稳定快照；任务不存在或已被回收时返回 `None`。
pub fn task_snapshot(task_id : TaskId) -> Option<TaskSnapshot> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| Some(scheduler.task_snapshot(task_id)))
}

/// 当前调度器逻辑 tick。
pub fn current_tick() -> TaskTick {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.current_tick())
}

/// 当前任务内核栈顶，供 trap/用户态恢复路径使用。
pub fn current_task_kernel_stack_top() -> Option<usize> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.current_task_kernel_stack_top(cpu::current_cpu_id()))
}

/// 返回当前运行任务的用户地址空间 token；`0` 表示回落到内核地址空间。
pub fn current_task_address_space_raw() -> usize {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.current_task_address_space_raw(cpu::current_cpu_id()))
}

pub fn current_task_user_aspace_ptr() -> usize {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.current_task_user_aspace_ptr(cpu::current_cpu_id()))
}

pub fn current_task_user_address_space_token() -> usize {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| {
        scheduler.current_task_user_address_space_token(cpu::current_cpu_id())
    })
}

pub fn current_task_trap_return_address_space_token() -> usize {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| {
        scheduler.current_task_trap_return_address_space_token(cpu::current_cpu_id())
    })
}

/// 判断指定任务是否仍有子任务。
pub fn has_child(parent_id : TaskId) -> bool {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.has_child(parent_id))
}

// =============================================================================
//  8. Trap 帧访问
// =============================================================================

/// 开始由 Rust 修改当前任务的权威 trap 上下文，返回可写指针（若尚无当前任务则为
/// `None`）。
pub fn begin_current_trap_frame_access(trap_frame : TaskTrapFrame) -> Option<*mut TaskTrapFrame> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| {
        scheduler.begin_current_trap_frame_access(trap_frame, cpu::current_cpu_id())
    })
}

/// 将 TCB 中保存的 trap 现场恢复到调用方缓冲区。
pub fn restore_current_trap_frame(trap_frame : &mut TaskTrapFrame) -> bool {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| {
        scheduler.restore_current_trap_frame(trap_frame, cpu::current_cpu_id())
    })
}

/// 返回指定 CPU 的调度状态快照。
pub fn cpu_snapshot(cpu_id : CpuId) -> Option<CpuSnapshot> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.cpu_snapshot(cpu_id))
}

/// 查询全部已配置 CPU 的状态快照，包含尚未 online 的 CPU。
pub fn cpu_states() -> Vec<(CpuId, CpuSnapshot)> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.cpu_states())
}

/// 将各 CPU 的 online 与当前任务状态写入内核日志。
pub fn print_cpu_states() {
    for (cpu_id, state) in cpu_states() {
        log::info!("[cpu] id={} online={} current={:?} idle={:?} user_aspace={} tick={}",
                   cpu_id.raw(),
                   state.online,
                   state.current_task_id,
                   state.idle_task_id,
                   state.current_address_space
                        .is_some(),
                   state.current_ticks);
    }
}

/// 查询指定任务当前在哪个 CPU 上运行。
pub fn running_cpu(task_id : TaskId) -> Option<CpuId> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.running_cpu(task_id))
}

/// 将指定 CPU 标记为 online。AP 完成初始化后调用。
pub fn set_cpu_online(cpu_id : CpuId) {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.set_cpu_online(cpu_id));
}

/// 指定唯一推进 sleep/wait 全局逻辑时间的 BSP。
///
/// 不能假设 BSP 恒为 hart 0：OpenSBI 会把实际 boot hart 作为入口参数传入。
pub fn set_timekeeper_cpu(cpu_id : CpuId) {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.set_timekeeper_cpu(cpu_id));
}

/// Snapshot of CPUs that completed scheduler bring-up.
pub fn online_cpu_mask() -> CpuMask {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.online_cpu_mask())
}

// =============================================================================
//  9. 调度策略
// =============================================================================

/// 应用调度策略变更（完整版：detach/入队/必要时 RescheduleNow）。
pub fn apply_sched_policy_change(task_id : TaskId,
                                 policy : SchedPolicy,
                                 priority : Priority)
                                 -> Result<(), SchedError> {
    let _guard = InterruptGuard::new();
    let action = with_scheduler(|scheduler| {
        scheduler.apply_sched_policy_change(task_id,
                                            policy,
                                            priority,
                                            cpu::current_cpu_id())
    })?;
    if action {
        suspend_current_and_run_next();
    }
    Ok(())
}
pub fn set_affinity(task_id : TaskId, mask : CpuMask) -> Result<(), SchedError> {
    let cpu_id = cpu::current_cpu_id();
    let (result, targets) = {
        let _guard = InterruptGuard::new();
        with_scheduler(|scheduler| {
            let result = scheduler.set_affinity(task_id, mask);
            let targets = scheduler.take_pending_reschedule_cpus();
            (result, targets)
        })
    };
    // SBI/IPI 调用绝不能发生在 scheduler 锁或中断 guard 的作用域内。
    dispatch_reschedules(targets, cpu_id);
    result
}
pub fn get_affinity(task_id : TaskId) -> Result<CpuMask, SchedError> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.get_affinity(task_id))
}

/// 设置线程级 nice 属性。
///
/// 更新线程级 nice；正在运行的 `SCHED_OTHER` 任务会在下一 tick 使用新权重。
pub fn set_nice(task_id : TaskId, nice : i8) -> Result<(), SchedError> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.set_nice(task_id, nice))
}

pub fn get_nice(task_id : TaskId) -> Result<i8, SchedError> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.get_nice(task_id))
}
pub fn policy(task_id : TaskId) -> Result<SchedPolicy, SchedError> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.policy(task_id))
}
pub fn priority(task_id : TaskId) -> Result<Priority, SchedError> {
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.priority(task_id))
}

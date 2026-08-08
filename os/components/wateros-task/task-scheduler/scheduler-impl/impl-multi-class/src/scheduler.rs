//! [`MultiClassScheduler`]：OTHER/BATCH/IDLE/FIFO/RR 五类调度。

extern crate alloc;
use alloc::boxed::Box;
use alloc::vec::Vec;
mod cpu;
mod policy;
mod query;
mod runqueue;
mod tasks;
mod wait;
use crate::{SwitchPair, TaskTrapFrame};
use api_v0::{CPUState, CpuSnapshot, QueueTarget, TaskRegistry, WaitQueues};
use arch::task::{ActiveArchTaskContext as TaskContext, ArchTaskContext};
use base::cpu::CpuMask;
use config::task::MAX_CPUS;
use task_api::{
    AddressSpaceHandle, CpuId, ExitedTask, KernelTaskEntry, Priority, SchedError, SchedPolicy,
    TaskExitCode, TaskId, TaskSnapshot, TaskState, TaskTick, TaskWaitResult, TaskWaitTarget,
    UserTask, WaitQueueId,
};

use api_v0::{RescheduleCause, ScheduleReason};

unsafe extern "C" {
    static kernel_heap_start: u8;
    static kernel_heap_end: u8;
}

pub(super) struct MultiClassScheduler {
    pub registry : TaskRegistry,
    pub wait_queues : WaitQueues,
    pub cpu_states : Box<[CPUState]>,
    /// 环形选核的起点。负载相同时，从这里开始的第一个 online CPU 获胜。
    pub next_placement_cpu : usize,
    /// 唯一推进全局 sleep/wait timeout 时间的 BSP。
    pub timekeeper_cpu : Option<CpuId>,
    /// 入队时在 scheduler 锁内累计，锁外再实际发送定向 IPI。
    pub pending_reschedule_cpus : CpuMask,
}

/// Ready 任务的放置偏好；最终目标仍须满足 online 与 affinity 约束。
#[derive(Clone, Copy)]
pub(super) enum ReadyPlacement {
    LeastLoaded,
    LastCpu,
}

impl MultiClassScheduler {
    // ================================================================
    //  构造与初始化
    // ================================================================
    pub(super) fn new() -> Self {
        let mut cpu_states = Vec::with_capacity(MAX_CPUS);
        for raw in 0..MAX_CPUS {
            cpu_states.push(CPUState::new(CpuId::from_raw(raw)));
        }
        Self { registry : TaskRegistry::new(),
               wait_queues : WaitQueues::new(),
               cpu_states : cpu_states.into_boxed_slice(),
               next_placement_cpu : 0,
               timekeeper_cpu : None,
               pending_reschedule_cpus : CpuMask::EMPTY }
    }

    pub(super) fn init(&mut self, boot_cpu : CpuId) {
        self.registry.init();
        self.wait_queues
            .init();
        self.next_placement_cpu = 0;
        self.timekeeper_cpu = None;
        self.pending_reschedule_cpus = CpuMask::EMPTY;
        // 为每个 CPU 创建 idle 任务
        for (cpu_id, cpu_state) in self.cpu_states
                                       .iter_mut()
                                       .enumerate()
        {
            let cpu_id = CpuId::from_raw(cpu_id);
            cpu_state.init(cpu_id);
            cpu_state.set_online(boot_cpu == cpu_id);
            let idle_id = self.registry
                              .spawn_idle_task();
            cpu_state.set_idle_task_id(idle_id);
            let idle_snapshot = self.registry
                                    .task_snapshot(idle_id);
            cpu_state.set_current_task(&idle_snapshot);
        }
    }

    // ================================================================
    //  核心调度
    // ================================================================

    /// 标记任务为 Running 并更新当前 CPU 的 current_task_id。
    /// CACHE_SYNC: TaskSnapshot → CPUState;  
    fn set_current_task(&mut self, snap : &TaskSnapshot, cpu_id : CpuId) {
        if let Some(running_cpu) = snap.running_cpu_id {
            assert_eq!(running_cpu,
                       cpu_id,
                       "[scheduler] task {} is already running on CPU {} while CPU {} selected it",
                       snap.id,
                       running_cpu.raw(),
                       cpu_id.raw());
        }
        let cpu_state = &mut self.cpu_states[cpu_id.raw()];
        let previous_aspace = cpu_state.current_aspace();
        if previous_aspace != snap.user_aspace_ptr {
            mm_api::user_aspace_lifecycle::notify_aspace_cpu_leave(previous_aspace, cpu_id);
            mm_api::user_aspace_lifecycle::notify_aspace_cpu_enter(snap.user_aspace_ptr, cpu_id);
        }
        cpu_state.set_current_task(snap);
        self.registry
            .mark_running(snap.id, cpu_id);
    }

    /// 在实际 `__switch` 前验证恢复目标上下文。返回地址落在 kernel heap
    /// 必然是上下文损坏：heap 数据绝不应被当作指令执行。
    fn validate_switch_target(&self, task_id : TaskId, cx : *const TaskContext, cpu_id : CpuId) {
        assert!(!cx.is_null(),
                "[scheduler] null next context: cpu={} task={}",
                cpu_id.raw(),
                task_id);
        let context = unsafe { &*cx };
        let ra = context.return_address();
        let sp = context.stack_pointer();
        let heap_start = core::ptr::addr_of!(kernel_heap_start) as usize;
        let heap_end = core::ptr::addr_of!(kernel_heap_end) as usize;
        if (heap_start..heap_end).contains(&ra) {
            panic!("[scheduler] corrupted switch target: cpu={} task={} cx={:#x} ra={:#x} \
                    sp={:#x} heap=[{:#x},{:#x})",
                   cpu_id.raw(),
                   task_id,
                   cx as usize,
                   ra,
                   sp,
                   heap_start,
                   heap_end);
        }
    }

    /// 构造切换对，并在唯一的出口处校验恢复目标。
    fn switch_pair(&self,
                   current : *mut TaskContext,
                   next_task_id : TaskId,
                   next : *const TaskContext,
                   cpu_id : CpuId)
                   -> SwitchPair {
        self.validate_switch_target(next_task_id, next, cpu_id);
        (current, next)
    }

    /// 首次任务切换（冷启动入口）。
    pub(super) fn prepare_first_switch(&mut self, cpu_id : CpuId) -> SwitchPair {
        self.cpu_states[cpu_id.raw()].leave_boot_context();
        let next_task_id = self.cpu_states[cpu_id.raw()].pick_next_runnable();
        let snap = self.registry
                       .task_snapshot(next_task_id);
        self.set_current_task(&snap, cpu_id);
        let boot_task_cx = self.cpu_states[cpu_id.raw() as usize].boot_task_cx();
        self.switch_pair(boot_task_cx,
                         next_task_id,
                         snap.task_cx as *const TaskContext,
                         cpu_id)
    }


    /// 普通调度入口：根据 `reason` 决定是否切换当前任务。
    pub(super) fn schedule(&mut self,
                           reason : ScheduleReason,
                           cpu_id : CpuId)
                           -> Option<SwitchPair> {
        // Phase 1: 根据 reason 做前置处理
        match reason {
            // The caller has already consumed `need_resched`. Rechecking with
            // Tick semantics here would discard a Forced request.
            ScheduleReason::Reschedule => {}
            ScheduleReason::Tick => self.tick(cpu_id)?,
            // Yield / Block / Sleep / Exit：在选下一个任务之前确保所有到期任务已入队
            _ => {
                if self.is_timekeeper_cpu(cpu_id) {
                    self.activate_woken_and_timeout_tasks();
                }
            }
        }

        // Phase 3: 从 cpu_states 取出当前任务
        let current_task_id = self.cpu_states[cpu_id.raw()].current_task_id()
                                                           .expect("current task must exist");
        let current_ptr = self.cpu_states[cpu_id.raw()].current_task_cx();

        // Phase 4: IDLE 特殊处理（不经过 enqueue）
        if self.cpu_states[cpu_id.raw()].is_current_idle() {
            let next_task_id = self.cpu_states[cpu_id.raw()].pick_next_runnable();
            let snap = self.registry
                           .task_snapshot(next_task_id);
            if next_task_id == current_task_id {
                self.set_current_task(&snap, cpu_id);
                return None;
            }
            self.set_current_task(&snap, cpu_id);
            let next_ptr = snap.task_cx as *const TaskContext;
            return Some(self.switch_pair(current_ptr,
                                         next_task_id,
                                         next_ptr,
                                         cpu_id));
        }
        // Phase 5-8: 非 IDLE 调度
        if matches!(reason,
                    ScheduleReason::Yield | ScheduleReason::Sleep(0))
        {
            self.cpu_states[cpu_id.raw()].prepare_yield();
        }
        let queue_target = self.pick_queue(reason);
        self.enqueue_task(queue_target, current_task_id, cpu_id);
        // 当前任务的状态转换可能唤醒其它任务（最典型是 Exit 唤醒父 runner）。
        // 必须在转换之后取 next；提前 pick 会错误地选 idle，并令 CPU cache 与
        // 实际将要恢复的任务脱节。本地无任务可跑时顺势尝试从其它核偷取。
        let next_task_id = self.cpu_states[cpu_id.raw()].pick_next_runnable();
        let snap = self.registry
                       .task_snapshot(next_task_id);
        // Phase 8: 决定是否 __switch
        let is_exit = matches!(reason, ScheduleReason::Exit(_));
        if next_task_id == current_task_id {
            if is_exit {
                panic!("[scheduler] exited task {} remained runnable on CPU {}",
                       current_task_id,
                       cpu_id.raw());
            }
            self.set_current_task(&snap, cpu_id);
            return None;
        }
        self.set_current_task(&snap, cpu_id);
        let next_ptr = snap.task_cx as *const TaskContext;
        Some(self.switch_pair(current_ptr,
                              next_task_id,
                              next_ptr,
                              cpu_id))
    }


    /// Tick 前置处理：推进时间、检查时间片与抢占条件。
    fn tick(&mut self, cpu_id : CpuId) -> Option<()> {
        // 1. 推进全局 tick
        if self.is_timekeeper_cpu(cpu_id) {
            self.wait_queues
                .tick();
        }
        // 2. 仅推进 CPU 本地缓存；任务统计会在离开 CPU 时统一回写 TCB。
        // 3. 推进当前任务的时间片/vruntime（由 CPUState::tick 按策略分发）
        self.cpu_states[cpu_id.raw()].tick();

        let needs_switch =
            self.cpu_states[cpu_id.raw()].cpu_should_reschedule(RescheduleCause::Tick);

        // 5. 处理唤醒/超时任务
        if needs_switch ||
           self.wait_queues
               .has_woken_or_timeout_tasks()
        {
            if self.is_timekeeper_cpu(cpu_id) {
                self.activate_woken_and_timeout_tasks();
            }
        }
        if needs_switch {
            Some(())
        } else {
            None
        }
    }
    fn pick_queue(&mut self, reason : ScheduleReason) -> QueueTarget {
        match reason {
            ScheduleReason::StartFirst |
            ScheduleReason::Yield |
            ScheduleReason::Tick |
            ScheduleReason::Reschedule => QueueTarget::Ready,
            ScheduleReason::Block(block_reason) => QueueTarget::Blocked(block_reason),
            ScheduleReason::Sleep(ticks) => {
                if ticks == 0 {
                    QueueTarget::Ready
                } else {
                    let wake_tick = self.wait_queues
                                        .current_tick()
                                        .saturating_add(ticks);
                    QueueTarget::Sleeping(wake_tick)
                }
            }
            ScheduleReason::Exit(exit_code) => QueueTarget::Exited(exit_code),
        }
    }
    /// Phase 5-8: 非 IDLE 任务的完整调度路径（确定去向 → 摘除 → 入队 → 选下一个）。
    /// 等待调度入口：当前任务因等待某个 `target` 而阻塞。
    ///
    /// 如果目标已经就绪（`wait_target_ready` 返回 true），则无需阻塞，直接返回 `None`。
    /// 否则将当前任务放入等待队列 + 可选的超时队列，然后切换到下一个就绪任务。
    pub(super) fn schedule_wait(&mut self,
                                target : TaskWaitTarget,
                                timeout_ticks : Option<TaskTick>,
                                cpu_id : CpuId)
                                -> Option<SwitchPair> {
        // ===== Phase 1: 前置处理 =====
        if self.is_timekeeper_cpu(cpu_id) {
            self.activate_woken_and_timeout_tasks();
        }

        // ===== Phase 2: 快速路径 — 目标已就绪，无需阻塞 =====
        if self.registry
               .wait_target_ready(target)
        {
            if let Some(current_task_id) = self.cpu_states[cpu_id.raw()].current_task_id() {
                self.registry
                    .finish_wait(current_task_id, TaskWaitResult::Woken);
            }
            return None;
        }

        // ===== Phase 3: 从 cpu_states 取出当前任务 =====
        let current_task_id = self.cpu_states[cpu_id.raw()].current_task_id()?;
        let current_ptr = self.cpu_states[cpu_id.raw()].current_task_cx();
        self.cpu_states[cpu_id.raw()].dequeue(current_task_id);

        // ===== Phase 4: 将当前任务入队到等待队列 =====
        self.enqueue_task(QueueTarget::Blocked(target),
                          current_task_id,
                          cpu_id);

        // ===== Phase 5: 可选超时 =====
        if let Some(timeout_ticks) = timeout_ticks {
            let wake_tick = self.wait_queues
                                .current_tick()
                                .saturating_add(timeout_ticks.max(1));
            self.wait_queues
                .enqueue_wait_timeout(current_task_id, target, wake_tick);
        }

        // ===== Phase 6: 选下一个任务，直接切换（当前已阻塞） =====
        let next_task_id = self.cpu_states[cpu_id.raw()].pick_next_runnable();
        let snap = self.registry
                       .task_snapshot(next_task_id);
        self.set_current_task(&snap, cpu_id);
        let next_ptr = snap.task_cx as *const TaskContext;
        Some(self.switch_pair(current_ptr,
                              next_task_id,
                              next_ptr,
                              cpu_id))
    }
}

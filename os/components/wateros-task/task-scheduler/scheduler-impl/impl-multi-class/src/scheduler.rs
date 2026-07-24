//! [`MultiClassScheduler`]：`SCHED_OTHER` + `SCHED_FIFO` + `SCHED_RR` 多类调度。

extern crate alloc;

use api_v0::{
    CPUScheduler, CpuSnapshot, FifoQueue, GlobalScheduler, QueueTarget, RrQueue, RrTickAction,
    SchedPolicyChangeAction,
};
use arch::task::ActiveArchTaskContext as TaskContext;
use base::cpu::CpuMask;
use config::task::MAX_CPUS;
use task_api::{
    AddressSpaceHandle, CpuId, ExitedTask, KernelTaskEntry, SchedError, SchedParam, SchedPolicy,
    TaskExitCode, TaskId, TaskSnapshot, TaskState, TaskTick, TaskWaitResult, TaskWaitTarget,
    UserTask, WaitQueueId, IDLE_TASK_ID,
};

use crate::{SwitchPair, TaskTrapFrame};

use api_v0::ScheduleReason;
pub(super) struct MultiClassScheduler {
    pub global : GlobalScheduler,
    pub cpu_states : [CPUScheduler; MAX_CPUS],
    /// 环形选核的起点。负载相同时，从这里开始的第一个 online CPU 获胜。
    pub next_placement_cpu : usize,
    /// 唯一推进全局 sleep/wait timeout 时间的 BSP。
    pub timekeeper_cpu : Option<CpuId>,
    /// 入队时在 scheduler 锁内累计，锁外再实际发送定向 IPI。
    pending_reschedule_cpus : CpuMask,
}

include!("scheduler/cpu.rs");
include!("scheduler/lifecycle.rs");
include!("scheduler/policy.rs");
include!("scheduler/tasks.rs");
include!("scheduler/wait.rs");

impl MultiClassScheduler {
    // ================================================================
    //  构造与初始化
    // ================================================================
    pub(super) fn new() -> Self {
        Self { global : GlobalScheduler::new(),
               cpu_states : core::array::from_fn(|i| CPUScheduler::new(CpuId::from_raw(i))),
               next_placement_cpu : 0,
               timekeeper_cpu : None,
               pending_reschedule_cpus : CpuMask::EMPTY }
    }

    pub(super) fn init_on_cpu(&mut self, boot_cpu : CpuId) {
        assert!(boot_cpu.fits_capacity(self.cpu_states
                                           .len()),
                "boot CPU is outside scheduler capacity");
        self.global.init();
        self.next_placement_cpu = 0;
        self.timekeeper_cpu = None;
        self.pending_reschedule_cpus = CpuMask::EMPTY;
        // 为每个 configured CPU 创建 idle 任务
        for cpu_id in 0..self.cpu_states
                             .len()
        {
            // 重置 per-CPU 队列（init 可重入）
            self.cpu_states[cpu_id].other_queue
                                   .init();
            self.cpu_states[cpu_id].fifo_queue = FifoQueue::new();
            self.cpu_states[cpu_id].rr_queue = RrQueue::new();
            self.cpu_states[cpu_id].online = cpu_id == boot_cpu.raw();
            self.cpu_states[cpu_id].need_resched = false;
            let idle_id = self.global
                              .registry
                              .spawn_idle_task();
            self.cpu_states[cpu_id].idle_task_id = Some(idle_id);
        }
    }

    // ================================================================
    //  核心调度
    // ================================================================

    /// 标记任务为 Running 并更新当前 CPU 的 current_task_id。
    fn set_current_task(&mut self, task_id : TaskId, cpu_id : CpuId) {
        let previous_task_id = self.cpu_states[cpu_id.raw()].current_task_id;
        if !self.global
                .registry
                .is_idle(task_id)
        {
            assert!(self.cpu_states
                        .iter()
                        .enumerate()
                        .all(|(index, cpu)| {
                            index == cpu_id.raw() || cpu.current_task_id != Some(task_id)
                        }),
                    "task is current on another CPU");
            assert_eq!(self.global
                           .registry
                           .ready_cpu_id(task_id),
                       Some(cpu_id),
                       "selected task is not owned by this CPU runqueue");
        }
        self.cpu_states[cpu_id.raw()].current_task_id = Some(task_id);
        if previous_task_id != Some(task_id) {
            self.cpu_states[cpu_id.raw()].context_switches =
                self.cpu_states[cpu_id.raw()].context_switches
                                             .saturating_add(1);
        }
        self.global
            .registry
            .mark_running(task_id, cpu_id);
        if !self.global
                .registry
                .is_idle(task_id)
        {
            assert_eq!(self.global
                           .registry
                           .running_cpu_id(task_id),
                       Some(cpu_id));
            assert_eq!(self.cpu_states
                           .iter()
                           .filter(|cpu| cpu.current_task_id == Some(task_id))
                           .count(),
                       1,
                       "task is current on more than one CPU");
        }
    }

    /// 首次任务切换（冷启动入口）。
    pub(super) fn prepare_first_switch(&mut self, cpu_id : CpuId) -> SwitchPair {
        let next_task_id = self.pick_next_runnable(cpu_id);
        self.set_current_task(next_task_id, cpu_id);
        (self.cpu_states[cpu_id.raw() as usize].boot_task_cx(),
         self.global
             .registry
             .task_cx_ptr(next_task_id))
    }

    /// 普通调度入口：根据 `reason` 决定是否切换当前任务。
    pub(super) fn schedule(&mut self,
                           reason : ScheduleReason,
                           cpu_id : CpuId)
                           -> Option<SwitchPair> {
        // ===== Phase 1: 根据 reason 做前置处理 =====
        match reason {
            ScheduleReason::Reschedule => {
                let Some(current_id) = self.cpu_states[cpu_id.raw()].current_task_id else {
                    return None;
                };
                let current = self.global
                                  .registry
                                  .task_snapshot(current_id);
                let current_affinity = self.global
                                           .registry
                                           .get_affinity(current_id)
                                           .expect("current task must exist");
                if current_affinity.contains(cpu_id) &&
                   !self.ready_task_should_preempt(current_id, current, cpu_id)
                {
                    return None;
                }
                self.cpu_states[cpu_id.raw()].other_queue
                                             .reset_ticks();
            }
            // --- Tick 路径：检查时间片与抢占条件 ---
            ScheduleReason::Tick => {
                // 1a. 推进全局 tick 和当前任务的 tick 计数
                self.cpu_states[cpu_id.raw()].timer_ticks =
                    self.cpu_states[cpu_id.raw()].timer_ticks
                                                 .saturating_add(1);
                // 全局逻辑时间只能由启动时登记的 BSP 推进。
                // AP timer tick 只处理本 CPU 的时间片消耗和本地抢占检查，
                // 不推进 wait_queues.on_tick()，否则多核下 sleep/wait timeout
                // 会以 1/N 的时间提前到期（N = online CPU 数）。
                if self.is_timekeeper_cpu(cpu_id) {
                    self.global
                        .wait_queues
                        .on_tick();
                }
                if let Some(id) = self.cpu_states[cpu_id.raw()].current_task_id {
                    self.global
                        .registry
                        .account_tick(id);
                }

                // 1b. 获取当前任务的 (id, snapshot)
                let current = self.cpu_states[cpu_id.raw()].current_task_id
                                                           .map(|task_id| {
                                                               (task_id,
                                                                self.global
                                                                    .registry
                                                                    .task_snapshot(task_id))
                                                           });

                // 1c. 判断任务所剩时间片是否耗尽（按策略分别处理）
                let quantum_expired = match current {
                    // 首次切换前尚未建立 current_task；正常运行时 idle 也会是 Some。
                    None => false,
                    Some((current_id, snap)) => match snap.sched_policy {
                        SchedPolicy::Other => self.cpu_states[cpu_id.raw()].other_queue
                                                                           .tick_current(),
                        SchedPolicy::Rr => matches!(self.cpu_states[cpu_id.raw()]
                                                        .rr_queue
                                                        .on_tick_current(current_id,
                                                                         snap.sched_priority),
                                                    RrTickAction::YieldToSamePriority),
                        SchedPolicy::Fifo => false,
                    },
                };

                // 1d. 判断就绪队列中是否有更高优先级的任务要抢占
                let ready_preempts = current.is_some_and(|(current_id, snap)| {
                                                self.ready_task_should_preempt(current_id, snap,
                                                                               cpu_id)
                                            });

                // 1e. 根据检查结果决定路径
                if quantum_expired || ready_preempts {
                    // AP 只处理本 CPU 的时间片；全局 timeout 仅由 timekeeper
                    // 在自己的 tick 中转为 Ready 任务。
                    if self.is_timekeeper_cpu(cpu_id) {
                        self.promote_sleep_and_timeouts();
                    }
                    self.cpu_states[cpu_id.raw()].other_queue
                                                 .reset_ticks();
                } else if self.is_timekeeper_cpu(cpu_id) &&
                          self.global
                              .wait_queues
                              .has_due_timers()
                {
                    self.promote_sleep_and_timeouts();
                    return None;
                } else {
                    return None;
                }
            }
            ScheduleReason::Sleep(ticks) if ticks == 0 => {
                return self.schedule(ScheduleReason::Yield, cpu_id);
            }
            _ => {
                self.cpu_states[cpu_id.raw()].other_queue
                                             .reset_ticks();
            }
        }

        // ===== Phase 2: 前置 promote（非 Tick 路径在此处理） =====
        if self.is_timekeeper_cpu(cpu_id) &&
           !matches!(reason,
                     ScheduleReason::Tick | ScheduleReason::Reschedule)
        {
            self.promote_sleep_and_timeouts();
        }

        // ===== Phase 3: 从 cpu_states 取出当前任务 =====
        let current_task_id =
            self.cpu_states[cpu_id.raw()].current_task_id
                                         .unwrap_or_else(|| {
                                             panic!("current task must exist: cpu_id={} \
                                                     reason={:?} online={} idle={:?}",
                                                    cpu_id.raw(),
                                                    reason,
                                                    self.cpu_states[cpu_id.raw()].online,
                                                    self.cpu_states[cpu_id.raw()].idle_task_id)
                                         });
        let current_ptr = self.global
                              .registry
                              .take_task_cx(current_task_id);
        // Sleep 路径额外清除旧的 wait_result
        if matches!(reason, ScheduleReason::Sleep(_)) {
            self.global
                .registry
                .clear_wait_result(current_task_id);
        }

        // ===== Phase 4: IDLE 任务特殊处理 =====
        // IDLE 不经过 enqueue（它不属于任何就绪队列），直接选下一个
        if self.global
               .registry
               .is_idle(current_task_id)
        {
            let next_task_id = self.pick_next_runnable(cpu_id);
            if next_task_id == current_task_id {
                self.set_current_task(next_task_id, cpu_id);
                return None;
            }
            let snap = self.global
                           .registry
                           .task_snapshot(next_task_id);
            if snap.sched_policy == SchedPolicy::Rr {
                self.cpu_states[cpu_id.raw()].rr_queue
                                             .note_running(next_task_id, snap.sched_priority);
            }
            self.set_current_task(next_task_id, cpu_id);
            let next_ptr = self.global
                               .registry
                               .task_cx_ptr(next_task_id);
            return Some((current_ptr, next_ptr));
        }

        // ===== Phase 5: 确定当前任务的去向（queue_target） =====
        let is_exit = matches!(reason, ScheduleReason::Exit(_));
        let queue_target = match reason {
            ScheduleReason::StartFirst => QueueTarget::Ready,
            ScheduleReason::Yield | ScheduleReason::Tick | ScheduleReason::Reschedule => {
                QueueTarget::Ready
            }
            ScheduleReason::Block(block_reason) => QueueTarget::Blocked(block_reason),
            ScheduleReason::Sleep(ticks) => {
                let wake_tick = self.global
                                    .wait_queues
                                    .current_tick()
                                    .saturating_add(ticks.max(1));
                QueueTarget::Sleeping(wake_tick)
            }
            ScheduleReason::Exit(exit_code) => QueueTarget::Exited(exit_code),
        };

        // ===== Phase 6: 将当前任务从就绪队列摘除（如果不回 Ready） =====
        if !matches!(queue_target, QueueTarget::Ready) {
            self.detach_from_run_queues(current_task_id, cpu_id);
        }

        // Yield/Tick 时清除 RR 的运行状态（如果当前是 RR 任务）
        if matches!(queue_target, QueueTarget::Ready) {
            let snap = self.global
                           .registry
                           .task_snapshot(current_task_id);
            if snap.sched_policy == SchedPolicy::Rr {
                self.cpu_states[cpu_id.raw()].rr_queue
                                             .clear_running();
            }
        }

        // ===== Phase 7: 将当前任务入队到目标队列 =====
        self.enqueue_task(queue_target, current_task_id, cpu_id);

        // ===== Phase 8: 从就绪队列选出下一个任务，决定是否需要 __switch =====
        self.finish_schedule_switch(current_task_id,
                                    current_ptr,
                                    is_exit,
                                    cpu_id)
    }

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
        self.cpu_states[cpu_id.raw()].other_queue
                                     .reset_ticks();
        if self.is_timekeeper_cpu(cpu_id) {
            self.promote_sleep_and_timeouts();
        }

        // ===== Phase 2: 快速路径 — 目标已就绪，无需阻塞 =====
        if self.global
               .registry
               .wait_target_ready(target)
        {
            if let Some(current_task_id) = self.cpu_states[cpu_id.raw()].current_task_id {
                self.global
                    .registry
                    .finish_wait(current_task_id, TaskWaitResult::Woken);
            }
            return None;
        }

        // ===== Phase 3: 从 cpu_states 取出当前任务 =====
        let current_task_id = self.cpu_states[cpu_id.raw()].current_task_id?;
        let current_ptr = self.global
                              .registry
                              .take_task_cx(current_task_id);
        self.global
            .registry
            .clear_wait_result(current_task_id);
        self.detach_from_run_queues(current_task_id, cpu_id);

        // ===== Phase 4: 将当前任务入队到等待队列 =====
        self.enqueue_task(QueueTarget::Blocked(target),
                          current_task_id,
                          cpu_id);

        // ===== Phase 5: 可选超时 =====
        if let Some(timeout_ticks) = timeout_ticks {
            let wake_tick = self.global
                                .wait_queues
                                .current_tick()
                                .saturating_add(timeout_ticks.max(1));
            self.global
                .wait_queues
                .enqueue_wait_timeout(current_task_id, target, wake_tick);
        }

        // ===== Phase 6: 选下一个任务，直接切换（当前已阻塞） =====
        let next_task_id = self.pick_next_runnable(cpu_id);
        let snap = self.global
                       .registry
                       .task_snapshot(next_task_id);
        if snap.sched_policy == SchedPolicy::Rr {
            self.cpu_states[cpu_id.raw()].rr_queue
                                         .note_running(next_task_id, snap.sched_priority);
        }
        self.set_current_task(next_task_id, cpu_id);
        let next_ptr = self.global
                           .registry
                           .task_cx_ptr(next_task_id);
        Some((current_ptr, next_ptr))
    }

    /// 选定下一个任务，决定是否需要 `__switch`。
    fn finish_schedule_switch(&mut self,
                              current_task_id : TaskId,
                              current_ptr : *mut TaskContext,
                              is_exit : bool,
                              cpu_id : CpuId)
                              -> Option<SwitchPair> {
        let next_task_id = self.pick_next_runnable(cpu_id);
        // 选出来的还是自己，就绪队列里只剩它自己
        if next_task_id == current_task_id {
            // 当前任务在退出 → 不是 IDLE 就强行切到 IDLE
            if is_exit {
                if !self.global
                        .registry
                        .is_idle(current_task_id)
                {
                    let idle_id = self.cpu_states[cpu_id.raw()].idle_task_id
                                                               .unwrap_or(IDLE_TASK_ID);
                    self.set_current_task(idle_id, cpu_id);
                    let next_ptr = self.global
                                       .registry
                                       .task_cx_ptr(idle_id);
                    return Some((current_ptr, next_ptr));
                }
                panic!("exit_current called on idle task — this should never happen");
            }
            // 选出了自己且非退出 → 重新标记为 Running，不切换
            let snap = self.global
                           .registry
                           .task_snapshot(next_task_id);
            if snap.sched_policy == SchedPolicy::Rr {
                self.cpu_states[cpu_id.raw()].rr_queue
                                             .note_running(next_task_id, snap.sched_priority);
            }
            self.set_current_task(next_task_id, cpu_id);
            return None;
        }
        // 选出不同任务 → 返回切换对，调用方执行 __switch
        let snap = self.global
                       .registry
                       .task_snapshot(next_task_id);
        if snap.sched_policy == SchedPolicy::Rr {
            self.cpu_states[cpu_id.raw()].rr_queue
                                         .note_running(next_task_id, snap.sched_priority);
        }
        self.set_current_task(next_task_id, cpu_id);
        let next_ptr = self.global
                           .registry
                           .task_cx_ptr(next_task_id);
        Some((current_ptr, next_ptr))
    }

    /// 按优先级从就绪队列中选择下一个可运行任务。
    fn pick_next_runnable(&mut self, cpu_id : CpuId) -> TaskId {
        // 1) RR 当前任务（时间片未用完）
        if let Some(current_id) = self.cpu_states[cpu_id.raw()].current_task_id {
            let snap = self.global
                           .registry
                           .task_snapshot(current_id);
            if snap.sched_policy == SchedPolicy::Rr &&
               self.cpu_states[cpu_id.raw()].rr_queue
                                            .should_continue_current(current_id,
                                                                     snap.sched_priority)
            {
                return current_id;
            }
        }
        // 2) FIFO → 3) RR，按优先级 99→1 穿插扫描
        for priority in (1..=99).rev() {
            if let Some(task_id) = self.cpu_states[cpu_id.raw()].fifo_queue
                                                                .pop_front_at_priority(priority)
            {
                self.cpu_states[cpu_id.raw()].rr_queue
                                             .clear_running();
                return task_id;
            }
            if let Some(task_id) = self.cpu_states[cpu_id.raw()].rr_queue
                                                                .pick_at_priority(priority)
            {
                return task_id;
            }
        }
        // 4) OTHER → 5) 当前 CPU 的 IDLE
        self.cpu_states[cpu_id.raw()].rr_queue
                                     .clear_running();
        self.cpu_states[cpu_id.raw()]
            .other_queue
            .pick_next_runnable_task_id()
            .unwrap_or(self.cpu_states[cpu_id.raw()].idle_task_id
                                                    .unwrap_or(IDLE_TASK_ID))
    }

    /// 在 scheduler 锁内将当前任务转换到目标状态。
    fn enqueue_task(&mut self, target : QueueTarget, current_task_id : TaskId, cpu_id : CpuId) {
        match target {
            QueueTarget::Ready => {
                // 通常 Yield/Tick 会回到当前 CPU；但若 affinity 在运行期间被
                // 改为排除当前 CPU，必须由本 CPU 的 Reschedule 路径把它放到
                // 允许的远端 runqueue，不能继续在禁止 CPU 上运行。
                let affinity = self.global
                                   .registry
                                   .get_affinity(current_task_id)
                                   .expect("current task must exist");
                let target_cpu = if affinity.contains(cpu_id) {
                    cpu_id
                } else {
                    self.pick_cpu_for_new_task(current_task_id)
                };
                self.enqueue_ready_by_cpu(current_task_id, target_cpu);
                if target_cpu != cpu_id {
                    self.request_reschedule(target_cpu);
                }
            }
            QueueTarget::Blocked(reason) => {
                self.global
                    .registry
                    .mark_blocking(current_task_id, reason);
                self.global
                    .wait_queues
                    .enqueue_wait_task(current_task_id, reason);
            }
            QueueTarget::Sleeping(wake_tick) => {
                self.global
                    .registry
                    .mark_sleeping(current_task_id, wake_tick);
                self.global
                    .wait_queues
                    .enqueue_sleep_task(current_task_id, wake_tick);
            }
            QueueTarget::Exited(exit_code) => {
                let waiters = self.global
                                  .wait_queues
                                  .wake_all_waiters_for_task_exit(current_task_id);
                // 唤醒所有等待当前任务退出的 waiters
                for waiter_id in &waiters {
                    self.global
                        .registry
                        .finish_wait(*waiter_id, TaskWaitResult::Woken);
                    self.enqueue_woken_task(*waiter_id);
                }
                // 唤醒等待当前任务的父任务
                if let Some(parent_id) = self.global
                                             .registry
                                             .parent_id(current_task_id)
                {
                    let child_waiters = self.global
                                            .wait_queues
                                            .wake_child_exit_waiters(parent_id);
                    for waiter_id in &child_waiters {
                        self.global
                            .registry
                            .finish_wait(*waiter_id, TaskWaitResult::Woken);
                        self.enqueue_woken_task(*waiter_id);
                    }
                }
                self.global
                    .wait_queues
                    .enqueue_exited_task(current_task_id);
                self.global
                    .registry
                    .mark_exited(current_task_id, exit_code);
            }
        }
    }
    /// 选出一个 CPU 来放置新创建的任务（fork/clone/spawn）。
    pub(super) fn enqueue_ready_task(&mut self, task_id : TaskId) {
        let picked_cpu = self.pick_cpu_for_new_task(task_id);
        self.enqueue_ready_by_cpu(task_id, picked_cpu);
        self.request_reschedule(picked_cpu);
    }

    pub(super) fn enqueue_ready_task_on_cpu(&mut self, task_id : TaskId, cpu_id : CpuId) {
        assert!(cpu_id.fits_capacity(self.cpu_states.len()), "invalid task target CPU");
        assert!(self.cpu_states[cpu_id.raw()].online, "target CPU is offline");
        self.enqueue_ready_by_cpu(task_id, cpu_id);
        self.request_reschedule(cpu_id);
    }


    /// 将已阻塞任务优先放回其上次运行的 online CPU。
    ///
    /// `last_cpu_id` 不可用时才回退到新任务的最小负载选核策略。
    pub(super) fn enqueue_woken_task(&mut self, task_id : TaskId) -> CpuId {
        let affinity = self.global
                           .registry
                           .get_affinity(task_id)
                           .expect("woken task must exist");
        let target = self.global
                         .registry
                         .last_cpu_id(task_id)
                         .filter(|cpu_id| {
                             cpu_id.fits_capacity(self.cpu_states
                                                      .len())
                         })
                         .filter(|cpu_id| self.cpu_states[cpu_id.raw()].online)
                         .filter(|cpu_id| affinity.contains(*cpu_id))
                         .unwrap_or_else(|| self.pick_cpu_for_new_task(task_id));
        self.enqueue_ready_by_cpu(task_id, target);
        self.request_reschedule(target);
        target
    }

    fn request_reschedule(&mut self, cpu_id : CpuId) {
        self.cpu_states[cpu_id.raw()].need_resched = true;
        self.pending_reschedule_cpus
            .insert(cpu_id);
    }

    pub(super) fn take_pending_reschedule_cpus(&mut self) -> CpuMask {
        let pending = self.pending_reschedule_cpus;
        self.pending_reschedule_cpus = CpuMask::EMPTY;
        pending
    }

    /// 消费当前 CPU 的重调度请求；SSIP 没有请求位时不应触发调度。
    pub(super) fn take_need_resched(&mut self, cpu_id : CpuId) -> bool {
        let need_resched = self.cpu_states[cpu_id.raw()].need_resched;
        self.cpu_states[cpu_id.raw()].need_resched = false;
        need_resched
    }
    fn enqueue_ready_by_cpu(&mut self, task_id : TaskId, cpu_id : CpuId) {
        assert!(!self.global
                     .registry
                     .is_idle(task_id),
                "idle task must not be placed on a ready queue");
        debug_assert!(self.cpu_states[cpu_id.raw()].online,
                      "ready task must target an online CPU");
        debug_assert!(self.global
                          .registry
                          .get_affinity(task_id)
                          .expect("queued task must exist")
                          .contains(cpu_id),
                      "ready task must target a CPU allowed by its affinity");
        if let Some(old_cpu_id) = self.global
                                      .registry
                                      .ready_cpu_id(task_id)
        {
            // 策略切换或防御性重复入队时，先根据 TCB 所记录的旧归属摘除。
            // 这样同一任务不会同时存在于两个 CPU 的 runqueue。
            self.remove_from_cpu_runqueue(task_id, old_cpu_id);
        }
        // Publish lifecycle state and queue ownership as one scheduler-lock
        // transaction.  No observer can see Ready without its target queue.
        self.global
            .registry
            .mark_ready(task_id, cpu_id);
        let snap = self.global
                       .registry
                       .task_snapshot(task_id);
        match snap.sched_policy {
            SchedPolicy::Other => self.cpu_states[cpu_id.raw()].other_queue
                                                               .enqueue_ready_task(task_id),
            SchedPolicy::Fifo => {
                self.cpu_states[cpu_id.raw()].fifo_queue
                                             .enqueue(task_id, snap.sched_priority)
            }
            SchedPolicy::Rr => {
                self.cpu_states[cpu_id.raw()].rr_queue
                                             .on_task_unblocked(task_id, snap.sched_priority)
            }
        }
        assert_eq!(self.global
                       .registry
                       .ready_cpu_id(task_id),
                   Some(cpu_id));
        assert_eq!(self.global
                       .registry
                       .running_cpu_id(task_id),
                   None);
    }

    fn remove_from_cpu_runqueue(&mut self, task_id : TaskId, cpu_id : CpuId) {
        self.cpu_states[cpu_id.raw()].other_queue
                                     .detach_task(task_id);
        self.cpu_states[cpu_id.raw()].fifo_queue
                                     .remove(task_id);
        self.cpu_states[cpu_id.raw()].rr_queue
                                     .remove(task_id);
    }

    fn detach_from_run_queues(&mut self, task_id : TaskId, cpu_id : CpuId) {
        // Ready 任务必须按 TCB 的实际归属摘除；当前运行任务没有
        // `ready_cpu_id`，此时才使用调用 CPU 来清理 RR 当前状态。
        let owner_cpu_id = self.global
                               .registry
                               .ready_cpu_id(task_id)
                               .unwrap_or(cpu_id);
        self.remove_from_cpu_runqueue(task_id, owner_cpu_id);
    }

    /// 从所有 CPU 的就绪队列摘除任务（用于 kill / discard 等跨 CPU 操作）。
    fn detach_from_all_cpus(&mut self, task_id : TaskId) {
        for cpu in &mut self.cpu_states {
            cpu.other_queue
               .detach_task(task_id);
            cpu.fifo_queue
               .remove(task_id);
            cpu.rr_queue
               .remove(task_id);
        }
    }

    /// 在所有 CPU 的 OtherQueue 上清理 version 表项（用于 reap / discard）。
    fn forget_task_on_all_cpus(&mut self, task_id : TaskId) {
        for cpu in &mut self.cpu_states {
            cpu.other_queue
               .forget_task(task_id);
        }
    }

    /// 推进到期睡眠/超时任务到就绪队列。
    fn promote_sleep_and_timeouts(&mut self) {
        for task_id in &self.global
                            .wait_queues
                            .promote_sleeping_tasks()
        {
            self.enqueue_woken_task(*task_id);
        }
        for (task_id, target) in &self.global
                                      .wait_queues
                                      .promote_wait_timeouts()
        {
            let still_waiting = matches!(
                self.global.registry.state(*task_id),
                Some(TaskState::Blocking(t)) if t == *target
            );
            if !still_waiting {
                continue;
            }
            self.global
                .registry
                .finish_wait(*task_id, TaskWaitResult::TimedOut);
            self.enqueue_woken_task(*task_id);
        }
    }

    /// 就绪队列中最高实时任务优先级（不含 IDLE）。
    fn highest_ready_rt_priority(&self, cpu_id : CpuId) -> Option<i32> {
        match (self.cpu_states[cpu_id.raw()].fifo_queue
                                            .highest_runnable_priority(),
               self.cpu_states[cpu_id.raw()].rr_queue
                                            .highest_ready_priority())
        {
            (Some(fifo), Some(rr)) => Some(fifo.max(rr)),
            (fifo, rr) => fifo.or(rr),
        }
    }
    ///当前 CPU 正在运行的任务，是否应该立刻被该 CPU 就绪队列里的任务抢占。
    fn ready_task_should_preempt(&self,
                                 current_id : TaskId,
                                 current : TaskSnapshot,
                                 cpu_id : CpuId)
                                 -> bool {
        if self.global
               .registry
               .is_idle(current_id)
        {
            return self.highest_ready_rt_priority(cpu_id)
                       .is_some() ||
                   self.cpu_states[cpu_id.raw()].other_queue
                                                .has_runnable();
        }
        match current.sched_policy {
            SchedPolicy::Other => self.highest_ready_rt_priority(cpu_id)
                                      .is_some(),
            SchedPolicy::Fifo | SchedPolicy::Rr => {
                self.highest_ready_rt_priority(cpu_id)
                    .is_some_and(|priority| priority > current.sched_priority)
            }
        }
    }
}

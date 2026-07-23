//! [`MultiClassScheduler`]：`SCHED_OTHER` + `SCHED_FIFO` + `SCHED_RR` 多类调度。

extern crate alloc;

use api_v0::{
    CPUScheduler, CpuSnapshot, FifoQueue, GlobalScheduler, QueueTarget, RrQueue, RrTickAction,
    SchedPolicyChangeAction,
};
use arch::task::ActiveArchTaskContext as TaskContext;
use config::task::MAX_CPUS;
use task_api::{
    AddressSpaceHandle, CpuId, ExitedTask, KernelTaskEntry, SchedError, SchedParam, SchedPolicy,
    TaskExitCode, TaskId, TaskSnapshot, TaskState, TaskTick, TaskWaitResult, TaskWaitTarget,
    UserTask, WaitQueueId, IDLE_TASK_ID,
};

use crate::{SwitchPair, TaskTrapFrame};

use api_v0::ScheduleReason;
pub(super) struct MultiClassScheduler {
    global : GlobalScheduler,
    cpu_states : [CPUScheduler; MAX_CPUS],
}

impl MultiClassScheduler {
    // ================================================================
    //  构造与初始化
    // ================================================================
    pub(super) fn new() -> Self {
        Self { global : GlobalScheduler::new(),
               cpu_states : core::array::from_fn(|i| CPUScheduler::new(CpuId::from_raw(i))) }
    }

    pub(super) fn init(&mut self) {
        self.global.init();
        // 为每个 configured CPU 创建 idle 任务
        for cpu_id in 0..self.cpu_states
                             .len()
        {
            // 重置 per-CPU 队列（init 可重入）
            self.cpu_states[cpu_id].other_queue
                                   .init();
            self.cpu_states[cpu_id].fifo_queue = FifoQueue::new();
            self.cpu_states[cpu_id].rr_queue = RrQueue::new();
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
        self.cpu_states[cpu_id.raw()].current_task_id = Some(task_id);
        self.global
            .registry
            .mark_running(task_id, cpu_id);
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
            // --- Tick 路径：检查时间片与抢占条件 ---
            ScheduleReason::Tick => {
                // 1a. 推进全局 tick 和当前任务的 tick 计数
                self.global
                    .wait_queues
                    .on_tick();
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

                // 1c. 判断时间片是否耗尽（按策略分别处理）
                let quantum_expired = match current {
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
                    // 需要重新调度 → promote + 清零时间片，继续往下
                    self.promote_sleep_and_timeouts(cpu_id);
                    self.cpu_states[cpu_id.raw()].other_queue
                                                 .reset_ticks();
                } else if self.global
                              .wait_queues
                              .has_due_timers()
                {
                    self.promote_sleep_and_timeouts(cpu_id);
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
        if !matches!(reason, ScheduleReason::Tick) {
            self.promote_sleep_and_timeouts(cpu_id);
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
            ScheduleReason::Yield | ScheduleReason::Tick => QueueTarget::Ready,
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
        self.promote_sleep_and_timeouts(cpu_id);

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

    /// Phase 7：将当前任务入队到目标队列（更新 TCB 状态后再入队）。
    fn enqueue_task(&mut self, target : QueueTarget, current_task_id : TaskId, cpu_id : CpuId) {
        match target {
            QueueTarget::Ready => {
                self.global
                    .registry
                    .mark_ready(current_task_id);
                self.enqueue_ready_by_policy(current_task_id, cpu_id);
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
                for waiter_id in &waiters {
                    self.global
                        .registry
                        .finish_wait(*waiter_id, TaskWaitResult::Woken);
                    self.global
                        .registry
                        .mark_ready(*waiter_id);
                    self.enqueue_ready_by_policy(*waiter_id, cpu_id);
                }
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
                        self.global
                            .registry
                            .mark_ready(*waiter_id);
                        self.enqueue_ready_by_policy(*waiter_id, cpu_id);
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

    fn enqueue_ready_by_policy(&mut self, task_id : TaskId, cpu_id : CpuId) {
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
    }

    fn detach_from_run_queues(&mut self, task_id : TaskId, cpu_id : CpuId) {
        self.cpu_states[cpu_id.raw()].other_queue
                                     .detach_task(task_id);
        self.cpu_states[cpu_id.raw()].fifo_queue
                                     .remove(task_id);
        self.cpu_states[cpu_id.raw()].rr_queue
                                     .remove(task_id);
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
    fn promote_sleep_and_timeouts(&mut self, cpu_id : CpuId) {
        for task_id in &self.global
                            .wait_queues
                            .promote_sleeping_tasks()
        {
            self.global
                .registry
                .mark_ready(*task_id);
            self.enqueue_ready_by_policy(*task_id, cpu_id);
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
            self.global
                .registry
                .mark_ready(*task_id);
            self.enqueue_ready_by_policy(*task_id, cpu_id);
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

    // ================================================================
    //  任务创建
    // ================================================================

    pub(super) fn spawn_kernel_task(&mut self,
                                    entry : KernelTaskEntry,
                                    arg : usize,
                                    cpu_id : CpuId)
                                    -> TaskId {
        let current_task_id = self.cpu_states[cpu_id.raw()].current_task_id;
        let task_id = self.global
                          .registry
                          .spawn_kernel_task(entry, arg, current_task_id);
        self.enqueue_ready_by_policy(task_id, cpu_id);
        task_id
    }

    pub(super) fn create_user_task_spec(&mut self, spec : UserTask, cpu_id : CpuId) -> TaskId {
        let current_task_id = self.cpu_states[cpu_id.raw()].current_task_id;
        self.global
            .registry
            .spawn_user_task_spec(spec, current_task_id)
    }

    /// 就绪入队（仅入队，不创建 TCB）。
    pub(super) fn enqueue_ready_task(&mut self, task_id : TaskId, cpu_id : CpuId) {
        self.enqueue_ready_by_policy(task_id, cpu_id);
    }

    pub(super) fn spawn_user_task_spec(&mut self, spec : UserTask, cpu_id : CpuId) -> TaskId {
        let task_id = self.create_user_task_spec(spec, cpu_id);
        self.enqueue_ready_task(task_id, cpu_id);
        task_id
    }

    // ================================================================
    //  fork / clone / exec
    // ================================================================

    pub(super) fn create_fork_child(&mut self,
                                    child_stack : usize,
                                    new_aspace_ptr : usize,
                                    new_satp : usize,
                                    cpu_id : CpuId)
                                    -> Option<TaskId> {
        let current_task_id = self.cpu_states[cpu_id.raw()].current_task_id?;
        self.global
            .registry
            .fork_current(child_stack,
                          new_aspace_ptr,
                          new_satp,
                          current_task_id)
    }

    pub(super) fn fork_current(&mut self,
                               child_stack : usize,
                               new_aspace_ptr : usize,
                               new_satp : usize,
                               cpu_id : CpuId)
                               -> Option<TaskId> {
        let child_id = self.create_fork_child(child_stack,
                                              new_aspace_ptr,
                                              new_satp,
                                              cpu_id)?;
        self.enqueue_ready_task(child_id, cpu_id);
        Some(child_id)
    }

    pub(super) fn create_clone_thread(&mut self,
                                      child_stack : usize,
                                      tls : usize,
                                      set_tls : bool,
                                      cpu_id : CpuId)
                                      -> Option<TaskId> {
        let parent_id = self.cpu_states[cpu_id.raw()].current_task_id?;
        self.global
            .registry
            .clone_current_thread(child_stack, tls, set_tls, parent_id)
    }

    pub(super) fn clone_current_thread(&mut self,
                                       child_stack : usize,
                                       tls : usize,
                                       set_tls : bool,
                                       cpu_id : CpuId)
                                       -> Option<TaskId> {
        let child_id = self.create_clone_thread(child_stack, tls, set_tls, cpu_id)?;
        self.enqueue_ready_task(child_id, cpu_id);
        Some(child_id)
    }

    pub(super) fn execve_current(&mut self,
                                 entry_pc : usize,
                                 sp : usize,
                                 argc : usize,
                                 argv : usize,
                                 envp : usize,
                                 satp : usize,
                                 user_aspace_ptr : usize,
                                 image_info : task_api::UserImageInfo,
                                 stack_info : task_api::UserStack,
                                 cpu_id : CpuId) {
        let current_id = self.cpu_states[cpu_id.raw()].current_task_id
                                                      .expect("execve requires a current task");
        self.global
            .registry
            .execve_current(entry_pc,
                            sp,
                            argc,
                            argv,
                            envp,
                            satp,
                            user_aspace_ptr,
                            image_info,
                            stack_info,
                            current_id);
    }

    // ================================================================
    //  任务销毁与回收
    // ================================================================

    pub(super) fn kill_task(&mut self, task_id : TaskId, exit_code : TaskExitCode) -> bool {
        if self.global
               .registry
               .is_idle(task_id)
        {
            return false;
        }
        if self.global
               .registry
               .state(task_id)
               .is_none()
        {
            return false;
        }
        if matches!(self.global
                        .registry
                        .state(task_id),
                    Some(TaskState::Exited(_)))
        {
            return true;
        }
        // 检查是否正在某 CPU 上运行
        if self.cpu_states
               .iter()
               .any(|c| c.current_task_id == Some(task_id))
        {
            return false;
        }
        self.detach_from_all_cpus(task_id);
        self.global
            .wait_queues
            .kill_task(task_id);
        self.global
            .registry
            .mark_exited(task_id, exit_code);
        true
    }

    pub(super) fn discard_unstarted_task(&mut self, task_id : TaskId) {
        self.detach_from_all_cpus(task_id);
        self.global
            .wait_queues
            .detach_task_from_run_queues(task_id);
        if self.global
               .registry
               .discard_task(task_id)
        {
            self.forget_task_on_all_cpus(task_id);
        }
    }

    pub(super) fn reap_exited_task(&mut self, task_id : TaskId) -> Option<ExitedTask> {
        let exited = self.global
                         .wait_queues
                         .reap_exited_task(&mut self.global.registry, task_id)?;
        self.forget_task_on_all_cpus(task_id);
        Some(exited)
    }

    pub(super) fn reap_one_exited_task(&mut self) -> Option<ExitedTask> {
        let exited = self.global
                         .wait_queues
                         .reap_one_exited_task(&mut self.global.registry)?;
        self.forget_task_on_all_cpus(exited.id);
        Some(exited)
    }

    pub(super) fn reap_one_exited_child(&mut self, parent_id : TaskId) -> Option<ExitedTask> {
        let task_id = self.global
                          .registry
                          .find_exited_child(parent_id)?;
        self.reap_exited_task(task_id)
    }

    // ================================================================
    //  等待队列操作
    // ================================================================

    pub(super) fn allocate_wait_queue(&mut self) -> WaitQueueId {
        self.global
            .wait_queues
            .allocate_wait_queue()
    }

    pub(super) fn try_release_wait_queue(&mut self, wait_queue_id : WaitQueueId) -> bool {
        self.global
            .wait_queues
            .try_release_wait_queue(wait_queue_id)
    }

    pub(super) fn wake_task(&mut self, task_id : TaskId, cpu_id : CpuId) -> bool {
        if !self.global
                .wait_queues
                .wake_task(task_id)
        {
            return false;
        }
        if self.global
               .registry
               .state(task_id)
               .is_none()
        {
            return false;
        }
        self.global
            .registry
            .finish_wait(task_id, TaskWaitResult::Woken);
        self.global
            .registry
            .mark_ready(task_id);
        self.enqueue_ready_by_policy(task_id, cpu_id);
        true
    }

    pub(super) fn interrupt_task(&mut self, task_id : TaskId, cpu_id : CpuId) -> bool {
        if !self.global
                .wait_queues
                .interrupt_task(task_id)
        {
            return false;
        }
        if self.global
               .registry
               .state(task_id)
               .is_none()
        {
            return false;
        }
        self.global
            .registry
            .finish_wait(task_id, TaskWaitResult::Interrupted);
        self.global
            .registry
            .mark_ready(task_id);
        self.enqueue_ready_by_policy(task_id, cpu_id);
        true
    }

    pub(super) fn block_task_manual(&mut self, task_id : TaskId, cpu_id : CpuId) {
        if self.global
               .registry
               .state(task_id)
               .is_none()
        {
            return;
        }
        self.detach_from_run_queues(task_id, cpu_id);
        self.global
            .registry
            .mark_blocking(task_id, TaskWaitTarget::Manual);
        self.global
            .wait_queues
            .block_task_manual(task_id);
    }

    pub(super) fn wake_child_exit_waiters(&mut self, parent_id : TaskId, cpu_id : CpuId) {
        let waiters = self.global
                          .wait_queues
                          .wake_child_exit_waiters(parent_id);
        for waiter_id in &waiters {
            self.global
                .registry
                .finish_wait(*waiter_id, TaskWaitResult::Woken);
            self.global
                .registry
                .mark_ready(*waiter_id);
            self.enqueue_ready_by_policy(*waiter_id, cpu_id);
        }
    }

    pub(super) fn wake_one_in_wait_queue(&mut self,
                                         wait_queue_id : WaitQueueId,
                                         cpu_id : CpuId)
                                         -> Option<TaskId> {
        let task_id = self.global
                          .wait_queues
                          .wake_one_in_wait_queue(wait_queue_id)?;
        if self.global
               .registry
               .state(task_id)
               .is_none()
        {
            return None;
        }
        self.global
            .registry
            .finish_wait(task_id, TaskWaitResult::Woken);
        self.global
            .registry
            .mark_ready(task_id);
        self.enqueue_ready_by_policy(task_id, cpu_id);
        Some(task_id)
    }

    pub(super) fn wake_all_in_wait_queue(&mut self,
                                         wait_queue_id : WaitQueueId,
                                         cpu_id : CpuId)
                                         -> usize {
        let task_ids = self.global
                           .wait_queues
                           .wake_all_in_wait_queue(wait_queue_id);
        let mut count = 0usize;
        for task_id in &task_ids {
            if self.global
                   .registry
                   .state(*task_id)
                   .is_none()
            {
                continue;
            }
            self.global
                .registry
                .finish_wait(*task_id, TaskWaitResult::Woken);
            self.global
                .registry
                .mark_ready(*task_id);
            self.enqueue_ready_by_policy(*task_id, cpu_id);
            count = count.saturating_add(1);
        }
        count
    }

    pub(super) fn requeue_wait_queue(&mut self,
                                     from_wait_queue_id : WaitQueueId,
                                     to_wait_queue_id : WaitQueueId,
                                     wake_count : usize,
                                     requeue_count : usize,
                                     cpu_id : CpuId)
                                     -> usize {
        let (woken, moved, changed) = self.global
                                          .wait_queues
                                          .requeue_wait_queue(from_wait_queue_id,
                                                              to_wait_queue_id,
                                                              wake_count,
                                                              requeue_count);
        for task_id in &woken {
            self.global
                .registry
                .finish_wait(*task_id, TaskWaitResult::Woken);
            self.global
                .registry
                .mark_ready(*task_id);
            self.enqueue_ready_by_policy(*task_id, cpu_id);
        }
        for (task_id, _from_id) in &moved {
            self.global
                .registry
                .mark_blocking(*task_id,
                               TaskWaitTarget::WaitQueue(to_wait_queue_id));
        }
        changed
    }

    // ================================================================
    //  调度策略变更
    // ================================================================

    pub(super) fn apply_sched_policy_change(&mut self,
                                            task_id : TaskId,
                                            policy : SchedPolicy,
                                            param : SchedParam,
                                            cpu_id : CpuId)
                                            -> Result<SchedPolicyChangeAction, SchedError> {
        if !self.global
                .registry
                .is_schedulable(task_id)
        {
            return Err(SchedError::NoSuchTask);
        }
        let old_snap = self.global
                           .registry
                           .task_snapshot(task_id);
        let was_ready = old_snap.state == TaskState::Ready;

        self.detach_from_all_cpus(task_id);
        if !self.global
                .registry
                .set_task_sched(task_id, policy, param.priority)
        {
            return Err(SchedError::NoSuchTask);
        }
        if was_ready {
            self.enqueue_ready_by_policy(task_id, cpu_id);
        }

        // 检查当前 CPU 上的任务是否被抢占
        if let Some(current_id) = self.cpu_states[cpu_id.raw()].current_task_id {
            if current_id != task_id {
                let new = self.global
                              .registry
                              .task_snapshot(task_id);
                let cur = self.global
                              .registry
                              .task_snapshot(current_id);
                if Self::beats_running(new.sched_policy,
                                       new.sched_priority,
                                       cur.sched_policy,
                                       cur.sched_priority)
                {
                    return Ok(SchedPolicyChangeAction::RescheduleNow);
                }
            }
        }
        Ok(SchedPolicyChangeAction::NoReschedule)
    }

    /// 判断 challenger 是否比 runner 优先级高（先比调度类，再比优先级）。
    fn beats_running(challenger_policy : SchedPolicy,
                     challenger_priority : i32,
                     runner_policy : SchedPolicy,
                     runner_priority : i32)
                     -> bool {
        let chal_class = match challenger_policy {
            SchedPolicy::Other => 0u8,
            SchedPolicy::Fifo | SchedPolicy::Rr => 1u8,
        };
        let run_class = match runner_policy {
            SchedPolicy::Other => 0u8,
            SchedPolicy::Fifo | SchedPolicy::Rr => 1u8,
        };
        if chal_class != run_class {
            return chal_class > run_class;
        }
        challenger_priority > runner_priority
    }

    // ================================================================
    //  查询接口
    // ================================================================

    /// 将指定 CPU 标记为 online。AP 完成初始化后调用。
    pub(super) fn set_cpu_online(&mut self, cpu_id : CpuId) {
        if !cpu_id.fits_capacity(self.cpu_states.len()) {
            log::warn!("[cpu] invalid CPU {} ignored", cpu_id.raw());
            return;
        }
        let cpu = &mut self.cpu_states[cpu_id.raw()];
        if cpu.online {
            log::warn!("[cpu] CPU {} already online, ignored",
                       cpu_id.raw());
            return;
        }
        cpu.online = true;
        log::info!("[cpu] CPU {} is now online",
                   cpu_id.raw());
    }

    pub(super) fn online_cpu_mask(&self) -> base::cpu::CpuMask {
        let mut mask = base::cpu::CpuMask::EMPTY;
        for cpu in &self.cpu_states {
            if cpu.online { mask.insert(cpu.cpu_id); }
        }
        mask
    }

    pub(super) fn current_task_id(&self, cpu_id : CpuId) -> Option<TaskId> {
        self.cpu_states[cpu_id.raw()].current_task_id
    }

    pub(super) fn current_task_snapshot(&self, cpu_id : CpuId) -> Option<TaskSnapshot> {
        Some(self.global
                 .registry
                 .task_snapshot(self.cpu_states[cpu_id.raw()].current_task_id?))
    }

    pub(super) fn task_snapshot(&self, task_id : TaskId) -> TaskSnapshot {
        self.global
            .registry
            .task_snapshot(task_id)
    }

    pub(super) fn has_child(&self, parent_id : TaskId) -> bool {
        self.global
            .registry
            .has_child(parent_id)
    }

    pub(super) fn current_tick(&self) -> TaskTick {
        self.global
            .wait_queues
            .current_tick()
    }

    pub(super) fn current_task_kernel_stack_top(&self, cpu_id : CpuId) -> Option<usize> {
        Some(self.global
                 .registry
                 .task_kernel_stack_top(self.cpu_states[cpu_id.raw()].current_task_id?))
    }

    pub(super) fn current_task_address_space_raw(&self, cpu_id : CpuId) -> usize {
        self.cpu_states[cpu_id.raw()].current_task_id
                                     .map(|id| {
                                         self.global
                                             .registry
                                             .current_task_address_space_raw(id)
                                     })
                                     .unwrap_or(0)
    }

    pub(super) fn current_task_user_aspace_ptr(&self, cpu_id : CpuId) -> usize {
        self.cpu_states[cpu_id.raw()].current_task_id
                                     .map(|id| {
                                         self.global
                                             .registry
                                             .current_task_user_aspace_ptr(id)
                                     })
                                     .unwrap_or(0)
    }

    pub(super) fn current_task_user_address_space_token(&self, cpu_id : CpuId) -> usize {
        self.current_task_address_space_raw(cpu_id)
    }

    pub(super) fn current_task_trap_return_address_space_token(&self, cpu_id : CpuId) -> usize {
        self.cpu_states[cpu_id.raw()].current_task_id
                                     .map(|id| {
                                         self.global
                                             .registry
                                             .current_task_trap_return_address_space_token(id)
                                     })
                                     .unwrap_or(0)
    }

    pub(super) fn begin_current_trap_frame_access(&mut self,
                                                  trap_frame : TaskTrapFrame,
                                                  cpu_id : CpuId)
                                                  -> Option<*mut TaskTrapFrame> {
        let task_id = self.cpu_states[cpu_id.raw()].current_task_id?;
        self.global
            .registry
            .begin_trap_frame_access(trap_frame, task_id)
    }

    pub(super) fn restore_current_trap_frame(&self,
                                             trap_frame : &mut TaskTrapFrame,
                                             cpu_id : CpuId)
                                             -> bool {
        let task_id = match self.cpu_states[cpu_id.raw()].current_task_id {
            Some(id) => id,
            None => return false,
        };
        self.global
            .registry
            .restore_trap_frame(trap_frame, task_id)
    }

    pub(super) fn take_current_wait_result(&mut self, cpu_id : CpuId) -> TaskWaitResult {
        let task_id =
            self.cpu_states[cpu_id.raw()].current_task_id
                                         .expect("wait result can only be taken for a running \
                                                  task");
        self.global
            .registry
            .take_current_wait_result(task_id)
    }
    pub fn cpu_snapshot(&self, cpu_id : CpuId) -> Option<CpuSnapshot> {
        let cpu = self.cpu_states.get(cpu_id.raw())?;
        Some(CpuSnapshot { cpu_id : cpu_id,
                           online : cpu.online,
                           current_task_id : cpu.current_task_id,
                           idle_task_id : cpu.idle_task_id,
                           current_address_space:
                               cpu.current_task_id
                                  .and_then(|id| {
                                      let raw = self.global
                                                    .registry
                                                    .current_task_address_space_raw(id);
                                      if raw != 0 {
                                          Some(AddressSpaceHandle::from_raw(raw))
                                      } else {
                                          None
                                      }
                                  }),
                           current_task_ticks : self.global
                                                    .wait_queues
                                                    .current_tick() })
    }
    pub fn running_cpu(&self, task_id : TaskId) -> Option<CpuId> {
        self.cpu_states
            .iter()
            .position(|c| c.current_task_id == Some(task_id))
            .map(|i| CpuId::from_raw(i))
    }
}

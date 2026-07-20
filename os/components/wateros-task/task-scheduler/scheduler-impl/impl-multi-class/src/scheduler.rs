//! [`MultiClassScheduler`]：`SCHED_OTHER` + `SCHED_FIFO` + `SCHED_RR` 多类调度。

extern crate alloc;

use crate::queues::OtherReadyQueue;
use crate::rt_fifo_queue::RtFifoRunQueue;
use crate::rt_rr_queue::{RrTickAction, RtRrRunQueue};
use api_v0::{QueueTarget, SchedPolicyChangeAction, TaskRegistry, WaitQueues};
use arch::task::ActiveArchTaskContext as TaskContext;
use task_api::{
    ExitedTask, KernelTaskEntry, SchedError, SchedParam, SchedPolicy, TaskExitCode, TaskId,
    TaskSnapshot, TaskState, TaskTick, TaskWaitResult, TaskWaitTarget, UserTask, WaitQueueId,
    IDLE_TASK_ID,
};

use crate::{SwitchPair, TaskTrapFrame};

use api_v0::ScheduleReason;

pub(super) struct MultiClassScheduler {
    registry : TaskRegistry,
    wait : WaitQueues,
    other_ready : OtherReadyQueue,
    fifo_ready : RtFifoRunQueue,
    rr_ready : RtRrRunQueue,
}

impl MultiClassScheduler {
    // ================================================================
    //  构造与初始化
    // ================================================================
    pub(super) fn new() -> Self {
        Self { registry : TaskRegistry::new(),
               wait : WaitQueues::new(),
               other_ready : OtherReadyQueue::new(),
               fifo_ready : RtFifoRunQueue::new(),
               rr_ready : RtRrRunQueue::new() }
    }

    pub(super) fn init(&mut self) {
        self.registry.init();
        self.wait.init();
        self.other_ready
            .init();
        self.fifo_ready = RtFifoRunQueue::new();
        self.rr_ready = RtRrRunQueue::new();
    }

    // ================================================================
    //  核心调度
    // ================================================================

    /// 首次任务切换（冷启动入口）。
    pub(super) fn prepare_first_switch(&mut self) -> SwitchPair {
        self.promote_sleep_and_timeouts();
        let next_task_id = self.pick_next_runnable();
        self.registry
            .first_switch_to(next_task_id)
    }

    /// 普通调度入口：根据 `reason` 决定是否切换当前任务。
    pub(super) fn schedule(&mut self, reason : ScheduleReason) -> Option<SwitchPair> {
        // ===== Phase 1: 根据 reason 做前置处理 =====
        match reason {
            // --- Tick 路径：检查时间片与抢占条件 ---
            ScheduleReason::Tick => {
                // 1a. 推进全局 tick 和当前任务的 tick 计数
                self.wait.on_tick();
                self.registry
                    .account_tick_for_current();

                // 1b. 获取当前任务的 (id, snapshot)
                let current = self.registry
                                  .current_task_id()
                                  .and_then(|task_id| {
                                      self.registry
                                          .task_snapshot(task_id)
                                          .map(|snap| (task_id, snap))
                                  });

                // 1c. 判断时间片是否耗尽（按策略分别处理）
                let quantum_expired = match current {
                    None => false,
                    Some((current_id, snap)) => match snap.sched_policy {
                        SchedPolicy::Other => self.other_ready
                                                  .tick_current(),
                        SchedPolicy::Rr => matches!(self.rr_ready
                                                        .on_tick_current(current_id,
                                                                         snap.sched_priority),
                                                    RrTickAction::YieldToSamePriority),
                        SchedPolicy::Fifo => false,
                    },
                };

                // 1d. 判断就绪队列中是否有更高优先级的任务要抢占
                let ready_preempts = current.is_some_and(|(current_id, snap)| {
                                                self.ready_task_should_preempt(current_id, snap)
                                            });

                // 1e. 根据检查结果决定路径
                if quantum_expired || ready_preempts {
                    // 需要重新调度 → promote + 清零时间片，继续往下
                    self.promote_sleep_and_timeouts();
                    self.other_ready
                        .reset_ticks();
                } else if self.wait
                              .has_due_timers()
                {
                    self.promote_sleep_and_timeouts();
                    return None;
                } else {
                    return None;
                }
            }
            ScheduleReason::Sleep(ticks) if ticks == 0 => {
                return self.schedule(ScheduleReason::Yield);
            }
            _ => {
                self.other_ready
                    .reset_ticks();
            }
        }

        // ===== Phase 2: 前置 promote（非 Tick 路径在此处理） =====
        if !matches!(reason, ScheduleReason::Tick) {
            self.promote_sleep_and_timeouts();
        }

        // ===== Phase 3: 从 registry 取出当前任务 =====
        // 返回 (current_task_id, current_task_context_ptr)
        // 如果取不到（没有当前任务）则直接返回 None
        let (current_task_id, current_ptr) = self.registry
                                                 .take_current_switch_out()?;

        // Sleep 路径额外清除旧的 wait_result
        if matches!(reason, ScheduleReason::Sleep(_)) {
            self.registry
                .clear_wait_result(current_task_id);
        }

        // ===== Phase 4: IDLE 任务特殊处理 =====
        // IDLE 不经过 enqueue（它不属于任何就绪队列），直接选下一个
        if self.registry
               .is_idle(current_task_id)
        {
            let next_task_id = self.pick_next_runnable();
            if next_task_id == current_task_id {
                let _ = self.registry
                            .mark_running_and_set_current(next_task_id);
                return None;
            }
            if let Some(snap) = self.registry
                                    .task_snapshot(next_task_id)
            {
                if snap.sched_policy == SchedPolicy::Rr {
                    self.rr_ready
                        .note_running(next_task_id, snap.sched_priority);
                }
            }
            let next_ptr = self.registry
                               .mark_running_and_set_current(next_task_id);
            return Some((current_ptr, next_ptr));
        }

        // ===== Phase 5: 确定当前任务的去向（queue_target） =====
        let is_exit = matches!(reason, ScheduleReason::Exit(_));
        let queue_target = match reason {
            ScheduleReason::StartFirst => QueueTarget::Ready,
            ScheduleReason::Yield | ScheduleReason::Tick => QueueTarget::Ready,
            ScheduleReason::Block(block_reason) => QueueTarget::Blocked(block_reason),
            ScheduleReason::Sleep(ticks) => {
                let wake_tick = self.wait
                                    .current_tick()
                                    .saturating_add(ticks.max(1));
                QueueTarget::Sleeping(wake_tick)
            }
            ScheduleReason::Exit(exit_code) => QueueTarget::Exited(exit_code),
        };

        // ===== Phase 6: 将当前任务从就绪队列摘除（如果不回 Ready） =====
        if !matches!(queue_target, QueueTarget::Ready) {
            self.detach_from_run_queues(current_task_id);
        }

        // Yield/Tick 时清除 RR 的运行状态（如果当前是 RR 任务）
        if matches!(queue_target, QueueTarget::Ready) {
            if let Some(snap) = self.registry
                                    .task_snapshot(current_task_id)
            {
                if snap.sched_policy == SchedPolicy::Rr {
                    self.rr_ready
                        .clear_running();
                }
            }
        }

        // ===== Phase 7: 将当前任务入队到目标队列 =====
        self.enqueue_task(queue_target, current_task_id);

        // ===== Phase 8: 从就绪队列选出下一个任务，决定是否需要 __switch =====
        self.finish_schedule_switch(current_task_id, current_ptr, is_exit)
    }

    /// 等待调度入口：当前任务因等待某个 `target` 而阻塞。
    ///
    /// 如果目标已经就绪（`wait_target_ready` 返回 true），则无需阻塞，直接返回 `None`。
    /// 否则将当前任务放入等待队列 + 可选的超时队列，然后切换到下一个就绪任务。
    pub(super) fn schedule_wait(&mut self,
                                target : TaskWaitTarget,
                                timeout_ticks : Option<TaskTick>)
                                -> Option<SwitchPair> {
        // ===== Phase 1: 前置处理 =====
        self.other_ready
            .reset_ticks();
        self.promote_sleep_and_timeouts();

        // ===== Phase 2: 快速路径 — 目标已就绪，无需阻塞 =====
        if self.registry
               .wait_target_ready(target)
        {
            if let Some(current_task_id) = self.registry
                                               .current_task_id()
            {
                self.registry
                    .finish_wait(current_task_id, TaskWaitResult::Woken);
            }
            return None;
        }

        // ===== Phase 3: 取出当前任务 =====
        let (current_task_id, current_ptr) = self.registry
                                                 .take_current_switch_out()?;
        self.registry
            .clear_wait_result(current_task_id);
        self.detach_from_run_queues(current_task_id);

        // ===== Phase 4: 将当前任务入队到等待队列 =====
        self.enqueue_task(QueueTarget::Blocked(target),
                          current_task_id);

        // ===== Phase 5: 可选超时 =====
        if let Some(timeout_ticks) = timeout_ticks {
            let wake_tick = self.wait
                                .current_tick()
                                .saturating_add(timeout_ticks.max(1));
            self.wait
                .enqueue_wait_timeout(current_task_id, target, wake_tick);
        }

        // ===== Phase 6: 选下一个任务，直接切换（当前已阻塞） =====
        let next_task_id = self.pick_next_runnable();
        if let Some(snap) = self.registry
                                .task_snapshot(next_task_id)
        {
            if snap.sched_policy == SchedPolicy::Rr {
                self.rr_ready
                    .note_running(next_task_id, snap.sched_priority);
            }
        }
        let next_ptr = self.registry
                           .mark_running_and_set_current(next_task_id);
        Some((current_ptr, next_ptr))
    }

    /// 选定下一个任务，决定是否需要 `__switch`。
    fn finish_schedule_switch(&mut self,
                              current_task_id : TaskId,
                              current_ptr : *mut TaskContext,
                              is_exit : bool)
                              -> Option<SwitchPair> {
        let next_task_id = self.pick_next_runnable();
        // 选出来的还是自己，就绪队列里只剩它自己
        if next_task_id == current_task_id {
            // 当前任务在退出 → 不是 IDLE 就强行切到 IDLE
            if is_exit {
                if !self.registry
                        .is_idle(current_task_id)
                {
                    let next_ptr = self.registry
                                       .mark_running_and_set_current(IDLE_TASK_ID);
                    return Some((current_ptr, next_ptr));
                }
                panic!("exit_current: no runnable task after exit");
            }
            // 选出了自己且非退出 → 重新标记为 Running，不切换
            if let Some(snap) = self.registry
                                    .task_snapshot(next_task_id)
            {
                if snap.sched_policy == SchedPolicy::Rr {
                    self.rr_ready
                        .note_running(next_task_id, snap.sched_priority);
                }
            }
            let _ = self.registry
                        .mark_running_and_set_current(next_task_id);
            return None;
        }
        // 选出不同任务 → 返回切换对，调用方执行 __switch
        if let Some(snap) = self.registry
                                .task_snapshot(next_task_id)
        {
            if snap.sched_policy == SchedPolicy::Rr {
                self.rr_ready
                    .note_running(next_task_id, snap.sched_priority);
            }
        }
        let next_ptr = self.registry
                           .mark_running_and_set_current(next_task_id);
        Some((current_ptr, next_ptr))
    }

    /// 按优先级从就绪队列中选择下一个可运行任务。
    fn pick_next_runnable(&mut self) -> TaskId {
        // 1) RR 当前任务（时间片未用完）
        if let Some(current_id) = self.registry
                                      .current_task_id()
        {
            if let Some(snap) = self.registry
                                    .task_snapshot(current_id)
            {
                if snap.sched_policy == SchedPolicy::Rr &&
                   self.rr_ready
                       .should_continue_current(current_id, snap.sched_priority)
                {
                    return current_id;
                }
            }
        }
        // 2) FIFO → 3) RR，按优先级 99→1 穿插扫描
        for priority in (1..=99).rev() {
            if let Some(task_id) = self.fifo_ready
                                       .pop_front_at_priority(priority, &self.registry)
            {
                self.rr_ready
                    .clear_running();
                return task_id;
            }
            if let Some(task_id) = self.rr_ready
                                       .pick_at_priority(priority, &self.registry)
            {
                return task_id;
            }
        }
        // 4) OTHER → 5) IDLE
        self.rr_ready
            .clear_running();
        self.other_ready
            .pick_next_runnable_task_id(&self.registry)
            .unwrap_or(IDLE_TASK_ID)
    }

    /// Phase 7：将当前任务入队到目标队列（更新 TCB 状态后再入队）。
    fn enqueue_task(&mut self, target : QueueTarget, current_task_id : TaskId) {
        match target {
            QueueTarget::Ready => {
                self.registry
                    .mark_ready(current_task_id);
                self.enqueue_ready_by_policy(current_task_id);
            }
            QueueTarget::Blocked(reason) => {
                self.registry
                    .mark_blocking(current_task_id, reason);
                self.wait
                    .enqueue_wait_task(current_task_id, reason);
            }
            QueueTarget::Sleeping(wake_tick) => {
                self.registry
                    .mark_sleeping(current_task_id, wake_tick);
                self.wait
                    .enqueue_sleep_task(current_task_id, wake_tick);
            }
            QueueTarget::Exited(exit_code) => {
                let waiters = self.wait
                                  .wake_all_waiters_for_task_exit(current_task_id);
                for waiter_id in &waiters {
                    self.registry
                        .finish_wait(*waiter_id, TaskWaitResult::Woken);
                    self.registry
                        .mark_ready(*waiter_id);
                    self.enqueue_ready_by_policy(*waiter_id);
                }
                if let Some(parent_id) = self.registry
                                             .parent_id(current_task_id)
                {
                    let child_waiters = self.wait
                                            .wake_child_exit_waiters(parent_id);
                    for waiter_id in &child_waiters {
                        self.registry
                            .finish_wait(*waiter_id, TaskWaitResult::Woken);
                        self.registry
                            .mark_ready(*waiter_id);
                        self.enqueue_ready_by_policy(*waiter_id);
                    }
                }
                self.wait
                    .enqueue_exited_task(current_task_id);
                self.registry
                    .mark_exited(current_task_id, exit_code);
            }
        }
    }

    fn enqueue_ready_by_policy(&mut self, task_id : TaskId) {
        let Some(snap) = self.registry
                             .task_snapshot(task_id)
        else {
            return;
        };
        match snap.sched_policy {
            SchedPolicy::Other => self.other_ready
                                      .enqueue_ready_task(task_id),
            SchedPolicy::Fifo => self.fifo_ready
                                     .enqueue(task_id, snap.sched_priority),
            SchedPolicy::Rr => self.rr_ready
                                   .on_task_unblocked(task_id, snap.sched_priority),
        }
    }

    fn detach_from_run_queues(&mut self, task_id : TaskId) {
        self.other_ready
            .detach_task(task_id);
        self.fifo_ready
            .remove(task_id);
        self.rr_ready
            .remove(task_id);
    }

    /// 推进到期睡眠/超时任务到就绪队列。
    fn promote_sleep_and_timeouts(&mut self) {
        for task_id in &self.wait
                            .promote_sleeping_tasks()
        {
            self.registry
                .mark_ready(*task_id);
            self.enqueue_ready_by_policy(*task_id);
        }
        for (task_id, target) in &self.wait
                                      .promote_wait_timeouts()
        {
            let still_waiting = matches!(
                self.registry.state(*task_id),
                Some(TaskState::Blocking(t)) if t == *target
            );
            if !still_waiting {
                continue;
            }
            self.registry
                .finish_wait(*task_id, TaskWaitResult::TimedOut);
            self.registry
                .mark_ready(*task_id);
            self.enqueue_ready_by_policy(*task_id);
        }
    }

    /// 就绪队列中最高实时任务优先级（不含 IDLE）。
    fn highest_ready_rt_priority(&self) -> Option<i32> {
        match (self.fifo_ready
                   .highest_runnable_priority(&self.registry),
               self.rr_ready
                   .highest_ready_priority(&self.registry))
        {
            (Some(fifo), Some(rr)) => Some(fifo.max(rr)),
            (fifo, rr) => fifo.or(rr),
        }
    }

    fn ready_task_should_preempt(&self, current_id : TaskId, current : TaskSnapshot) -> bool {
        if self.registry
               .is_idle(current_id)
        {
            return self.highest_ready_rt_priority()
                       .is_some() ||
                   self.other_ready
                       .has_runnable(&self.registry);
        }
        match current.sched_policy {
            SchedPolicy::Other => self.highest_ready_rt_priority()
                                      .is_some(),
            SchedPolicy::Fifo | SchedPolicy::Rr => {
                self.highest_ready_rt_priority()
                    .is_some_and(|priority| priority > current.sched_priority)
            }
        }
    }

    // ================================================================
    //  任务创建
    // ================================================================

    pub(super) fn spawn_kernel_task(&mut self, entry : KernelTaskEntry, arg : usize) -> TaskId {
        let task_id = self.registry
                          .spawn_kernel_task(entry, arg);
        self.enqueue_ready_by_policy(task_id);
        task_id
    }

    pub(super) fn create_user_task_spec(&mut self, spec : UserTask) -> TaskId {
        self.registry
            .spawn_user_task_spec(spec)
    }

    /// 就绪入队（仅入队，不创建 TCB）。
    pub(super) fn enqueue_ready_task(&mut self, task_id : TaskId) {
        self.enqueue_ready_by_policy(task_id);
    }

    pub(super) fn spawn_user_task_spec(&mut self, spec : UserTask) -> TaskId {
        let task_id = self.create_user_task_spec(spec);
        self.enqueue_ready_task(task_id);
        log::debug!("[task-scheduler] spawned user task {}",
                    task_id);
        task_id
    }

    // ================================================================
    //  fork / clone / exec
    // ================================================================

    pub(super) fn create_fork_child(&mut self,
                                    child_stack : usize,
                                    new_aspace_ptr : usize,
                                    new_satp : usize)
                                    -> Option<TaskId> {
        self.registry
            .fork_current(child_stack, new_aspace_ptr, new_satp)
    }

    pub(super) fn fork_current(&mut self,
                               child_stack : usize,
                               new_aspace_ptr : usize,
                               new_satp : usize)
                               -> Option<TaskId> {
        let child_id = self.create_fork_child(child_stack, new_aspace_ptr, new_satp)?;
        self.enqueue_ready_task(child_id);
        Some(child_id)
    }

    pub(super) fn create_clone_thread(&mut self,
                                      child_stack : usize,
                                      tls : usize,
                                      set_tls : bool)
                                      -> Option<TaskId> {
        self.registry
            .clone_current_thread(child_stack, tls, set_tls)
    }

    pub(super) fn clone_current_thread(&mut self,
                                       child_stack : usize,
                                       tls : usize,
                                       set_tls : bool)
                                       -> Option<TaskId> {
        let child_id = self.create_clone_thread(child_stack, tls, set_tls)?;
        self.enqueue_ready_task(child_id);
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
                                 stack_info : task_api::UserStack) {
        self.registry
            .execve_current(entry_pc,
                            sp,
                            argc,
                            argv,
                            envp,
                            satp,
                            user_aspace_ptr,
                            image_info,
                            stack_info);
    }

    // ================================================================
    //  任务销毁与回收
    // ================================================================

    pub(super) fn kill_task(&mut self, task_id : TaskId, exit_code : TaskExitCode) -> bool {
        if self.registry
               .is_idle(task_id)
        {
            return false;
        }
        if self.registry
               .state(task_id)
               .is_none()
        {
            return false;
        }
        if matches!(self.registry
                        .state(task_id),
                    Some(TaskState::Exited(_)))
        {
            return true;
        }
        if self.registry
               .current_task_id() ==
           Some(task_id)
        {
            return false;
        }
        self.detach_from_run_queues(task_id);
        self.wait
            .kill_task(task_id);
        self.registry
            .mark_exited(task_id, exit_code);
        true
    }

    pub(super) fn discard_unstarted_task(&mut self, task_id : TaskId) {
        self.detach_from_run_queues(task_id);
        self.wait
            .detach_task_from_run_queues(task_id);
        if self.registry
               .discard_task(task_id)
        {
            self.other_ready
                .forget_task(task_id);
        }
    }

    pub(super) fn reap_exited_task(&mut self, task_id : TaskId) -> Option<ExitedTask> {
        let exited = self.wait
                         .reap_exited_task(&mut self.registry, task_id)?;
        self.other_ready
            .forget_task(task_id);
        Some(exited)
    }

    pub(super) fn reap_one_exited_task(&mut self) -> Option<ExitedTask> {
        let exited = self.wait
                         .reap_one_exited_task(&mut self.registry)?;
        self.other_ready
            .forget_task(exited.id);
        Some(exited)
    }

    pub(super) fn reap_one_exited_child(&mut self, parent_id : TaskId) -> Option<ExitedTask> {
        let task_id = self.registry
                          .find_exited_child(parent_id)?;
        self.reap_exited_task(task_id)
    }

    // ================================================================
    //  等待队列操作
    // ================================================================

    pub(super) fn allocate_wait_queue(&mut self) -> WaitQueueId {
        self.wait
            .allocate_wait_queue()
    }

    pub(super) fn try_release_wait_queue(&mut self, wait_queue_id : WaitQueueId) -> bool {
        self.wait
            .try_release_wait_queue(wait_queue_id)
    }

    pub(super) fn wake_task(&mut self, task_id : TaskId) -> bool {
        if !self.wait
                .wake_task(task_id)
        {
            return false;
        }
        if self.registry
               .state(task_id)
               .is_none()
        {
            return false;
        }
        self.registry
            .finish_wait(task_id, TaskWaitResult::Woken);
        self.registry
            .mark_ready(task_id);
        self.enqueue_ready_by_policy(task_id);
        true
    }

    pub(super) fn interrupt_task(&mut self, task_id : TaskId) -> bool {
        if !self.wait
                .interrupt_task(task_id)
        {
            return false;
        }
        if self.registry
               .state(task_id)
               .is_none()
        {
            return false;
        }
        self.registry
            .finish_wait(task_id, TaskWaitResult::Interrupted);
        self.registry
            .mark_ready(task_id);
        self.enqueue_ready_by_policy(task_id);
        true
    }

    pub(super) fn block_task_manual(&mut self, task_id : TaskId) {
        if self.registry
               .state(task_id)
               .is_none()
        {
            return;
        }
        self.detach_from_run_queues(task_id);
        self.registry
            .mark_blocking(task_id, TaskWaitTarget::Manual);
        self.wait
            .block_task_manual(task_id);
    }

    pub(super) fn wake_child_exit_waiters(&mut self, parent_id : TaskId) {
        let waiters = self.wait
                          .wake_child_exit_waiters(parent_id);
        for waiter_id in &waiters {
            self.registry
                .finish_wait(*waiter_id, TaskWaitResult::Woken);
            self.registry
                .mark_ready(*waiter_id);
            self.enqueue_ready_by_policy(*waiter_id);
        }
    }

    pub(super) fn wake_one_in_wait_queue(&mut self, wait_queue_id : WaitQueueId) -> Option<TaskId> {
        let task_id = self.wait
                          .wake_one_in_wait_queue(wait_queue_id)?;
        if self.registry
               .state(task_id)
               .is_none()
        {
            return None;
        }
        self.registry
            .finish_wait(task_id, TaskWaitResult::Woken);
        self.registry
            .mark_ready(task_id);
        self.enqueue_ready_by_policy(task_id);
        Some(task_id)
    }

    pub(super) fn wake_all_in_wait_queue(&mut self, wait_queue_id : WaitQueueId) -> usize {
        let task_ids = self.wait
                           .wake_all_in_wait_queue(wait_queue_id);
        let mut count = 0usize;
        for task_id in &task_ids {
            if self.registry
                   .state(*task_id)
                   .is_none()
            {
                continue;
            }
            self.registry
                .finish_wait(*task_id, TaskWaitResult::Woken);
            self.registry
                .mark_ready(*task_id);
            self.enqueue_ready_by_policy(*task_id);
            count = count.saturating_add(1);
        }
        count
    }

    pub(super) fn requeue_wait_queue(&mut self,
                                     from_wait_queue_id : WaitQueueId,
                                     to_wait_queue_id : WaitQueueId,
                                     wake_count : usize,
                                     requeue_count : usize)
                                     -> usize {
        let (woken, moved, changed) = self.wait
                                          .requeue_wait_queue(from_wait_queue_id,
                                                              to_wait_queue_id,
                                                              wake_count,
                                                              requeue_count);
        for task_id in &woken {
            self.registry
                .finish_wait(*task_id, TaskWaitResult::Woken);
            self.registry
                .mark_ready(*task_id);
            self.enqueue_ready_by_policy(*task_id);
        }
        for (task_id, _from_id) in &moved {
            self.registry
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
                                            param : SchedParam)
                                            -> Result<SchedPolicyChangeAction, SchedError> {
        let old_snap = self.registry
                           .task_snapshot(task_id)
                           .ok_or(SchedError::NoSuchTask)?;
        let was_ready = old_snap.state == TaskState::Ready;

        self.detach_from_run_queues(task_id);
        if !self.registry
                .set_task_sched(task_id, policy, param.priority)
        {
            return Err(SchedError::NoSuchTask);
        }
        if was_ready {
            self.enqueue_ready_by_policy(task_id);
        }

        if let Some(current_id) = self.registry
                                      .current_task_id()
        {
            if current_id != task_id {
                let new = self.registry
                              .task_snapshot(task_id)
                              .expect("task exists");
                let cur = self.registry
                              .task_snapshot(current_id)
                              .expect("current exists");
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

    pub(super) fn current_task_id(&self) -> Option<TaskId> {
        self.registry
            .current_task_id()
    }

    pub(super) fn current_task_snapshot(&self) -> Option<TaskSnapshot> {
        self.registry
            .current_task_snapshot()
    }

    pub(super) fn task_snapshot(&self, task_id : TaskId) -> Option<TaskSnapshot> {
        self.registry
            .task_snapshot(task_id)
    }

    pub(super) fn has_child(&self, parent_id : TaskId) -> bool {
        self.registry
            .has_child(parent_id)
    }

    pub(super) fn current_tick(&self) -> TaskTick {
        self.wait
            .current_tick()
    }

    pub(super) fn current_task_kernel_stack_top(&self) -> Option<usize> {
        self.registry
            .current_task_kernel_stack_top()
    }

    pub(super) fn current_task_address_space_raw(&self) -> usize {
        self.registry
            .current_task_address_space_raw()
    }

    pub(super) fn current_task_user_aspace_ptr(&self) -> usize {
        self.registry
            .current_task_user_aspace_ptr()
    }

    pub(super) fn current_task_user_address_space_token(&self) -> usize {
        self.registry
            .current_task_user_address_space_token()
    }

    pub(super) fn current_task_trap_return_address_space_token(&self) -> usize {
        self.registry
            .current_task_trap_return_address_space_token()
    }

    pub(super) fn begin_current_trap_frame_access(&mut self,
                                                  trap_frame : TaskTrapFrame)
                                                  -> Option<*mut TaskTrapFrame> {
        self.registry
            .begin_current_trap_frame_access(trap_frame)
    }

    pub(super) fn restore_current_trap_frame(&self, trap_frame : &mut TaskTrapFrame) -> bool {
        self.registry
            .restore_current_trap_frame(trap_frame)
    }

    pub(super) fn take_current_wait_result(&mut self) -> TaskWaitResult {
        self.registry
            .take_current_wait_result()
    }
}

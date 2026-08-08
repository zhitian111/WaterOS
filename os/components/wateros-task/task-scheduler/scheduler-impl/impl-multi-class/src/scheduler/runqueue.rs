// 就绪队列：放置/入队/出队与重调度记账。
use super::*;
impl MultiClassScheduler {
    /// TCB_SYNC: sync_current_to_registry → Registry 写回
    ///
    /// 这里只回写当前 CPU cache；目标 runqueue 的 vruntime 归一化只能在
    /// `enqueue_ready_on_cpu` 中按目标 CPU 的 baseline 完成。
    pub(super) fn sync_current_to_registry(&mut self, cpu_id : CpuId) {
        let (current_task_id, policy, vruntime, runtime_ticks) = {
            let cpu = &mut self.cpu_states[cpu_id.raw()];
            let Some(current_task_id) = cpu.current_task_id() else {
                return;
            };
            let values = (current_task_id,
                          cpu.current_policy(),
                          cpu.current_vruntime(),
                          cpu.current_runtime_ticks);
            cpu.current_runtime_ticks = 0;
            values
        };
        if CPUState::is_cfs_policy(policy) {
            self.registry
                .set_vruntime(current_task_id, vruntime);
        }
        self.registry
            .add_ticks(current_task_id, runtime_ticks);
    }
    /// 在 scheduler 锁内将当前任务转换到目标状态。
    pub(super) fn enqueue_task(&mut self,
                               target : QueueTarget,
                               current_task_id : TaskId,
                               cpu_id : CpuId) {
        self.sync_current_to_registry(cpu_id);
        match target {
            // Yield/Tick 后优先留在本核；affinity 不允许时延迟到 `__switch` 之后再迁移，
            // 避免把“仍在运行”的当前任务发布到别核被空闲核偷取（双核同跑）。
            QueueTarget::Ready => {
                let stay = self.registry
                               .get_affinity(current_task_id)
                               .expect("current task must exist in registry")
                               .contains(cpu_id);
                if stay {
                    self.activate_ready_task(current_task_id, ReadyPlacement::LastCpu);
                } else {
                    self.cpu_states[cpu_id.raw()].set_deferred_ready(current_task_id);
                }
            }
            QueueTarget::Blocked(reason) => {
                self.registry
                    .clear_wait_result(current_task_id);
                self.registry
                    .mark_blocking(current_task_id, reason);
                self.wait_queues
                    .enqueue_wait_task(current_task_id, reason);
            }
            QueueTarget::Sleeping(wake_tick) => {
                self.registry
                    .clear_wait_result(current_task_id);
                self.registry
                    .mark_sleeping(current_task_id, wake_tick);
                self.wait_queues
                    .enqueue_sleep_task(current_task_id, wake_tick);
            }
            QueueTarget::Exited(exit_code) => {
                let waiters = self.wait_queues
                                  .wake_all_waiters_for_task_exit(current_task_id);
                // 唤醒所有等待当前任务退出的 waiters
                for waiter_id in &waiters {
                    self.registry
                        .finish_wait(*waiter_id, TaskWaitResult::Woken);
                    self.activate_ready_task(*waiter_id, ReadyPlacement::LastCpu);
                }
                // 唤醒等待当前任务的父任务
                if let Some(parent_id) = self.registry
                                             .task_snapshot(current_task_id)
                                             .parent_id
                {
                    let child_waiters = self.wait_queues
                                            .wake_child_exit_waiters(parent_id);
                    for waiter_id in &child_waiters {
                        self.registry
                            .finish_wait(*waiter_id, TaskWaitResult::Woken);
                        self.activate_ready_task(*waiter_id, ReadyPlacement::LastCpu);
                    }
                }
                self.wait_queues
                    .enqueue_exited_task(current_task_id);
                self.registry
                    .mark_exited(current_task_id, exit_code);
            }
        }
    }
    /// 源 CPU 已通过 `__switch` 保存完离开任务的上下文后，才把它发布到
    /// affinity 允许的 runqueue（由 `switch_and_unlock` 与首次任务入口在切走后调用）。
    pub(crate) fn enqueue_deferred_task(&mut self, cpu_id : CpuId) {
        let Some(task_id) = self.cpu_states[cpu_id.raw()].take_deferred_ready() else {
            return;
        };
        self.activate_ready_task(task_id, ReadyPlacement::LeastLoaded);
    }
    /// 激活非当前任务：选核、入 ready queue，并按统一 CPU 抢占规则请求调度。
    pub(crate) fn activate_ready_task(&mut self,
                                      task_id : TaskId,
                                      placement : ReadyPlacement)
                                      -> CpuId {
        let target = self.pick_ready_cpu(task_id, placement);
        self.enqueue_ready_on_cpu(task_id, target);
        let policy = self.registry
                         .task_snapshot(task_id)
                         .policy;
        self.request_reschedule(target, RescheduleCause::Ready(policy));
        target
    }

    pub(super) fn pick_ready_cpu(&mut self, task_id : TaskId, placement : ReadyPlacement) -> CpuId {
        let snap = self.registry
                       .task_snapshot(task_id);
        let preferred = match placement {
            ReadyPlacement::LeastLoaded => None,
            // 唤醒亲和性放宽：last_cpu 过载时，把任务放到更空的核。
            ReadyPlacement::LastCpu => snap.last_cpu_id
                                           .filter(|cpu_id| !self.cpu_is_overloaded(*cpu_id)),
        };
        if let Some(cpu_id) = preferred.filter(|cpu_id| {
                                           cpu_id.fits_capacity(self.cpu_states
                                                                    .len())
                                       })
                                       .filter(|cpu_id| self.cpu_states[cpu_id.raw()].online)
                                       .filter(|cpu_id| {
                                           snap.affinity
                                               .contains(*cpu_id)
                                       })
        {
            return cpu_id;
        }

        // 从环形起点开始选择负载最小的可用 CPU，避免相同负载长期偏向 CPU 0。
        let mut best_cpu = None;
        let mut min_load = usize::MAX;
        for offset in 0..self.cpu_states
                             .len()
        {
            let index = (self.next_placement_cpu + offset) %
                        self.cpu_states
                            .len();
            let cpu_id = CpuId::from_raw(index);
            if !self.cpu_states[index].online ||
               !snap.affinity
                    .contains(cpu_id)
            {
                continue;
            }
            let load = self.cpu_load(cpu_id);
            if load < min_load {
                min_load = load;
                best_cpu = Some(cpu_id);
            }
        }
        let cpu_id = best_cpu.expect("cannot enqueue a task without an online CPU");
        self.next_placement_cpu = (cpu_id.raw() + 1) %
                                  self.cpu_states
                                      .len();
        cpu_id
    }

    /// 根据 CPU 的统一抢占规则记录一次异步重调度请求。
    pub(crate) fn request_reschedule(&mut self, cpu_id : CpuId, cause : RescheduleCause) {
        if !self.cpu_states[cpu_id.raw()].cpu_should_reschedule(cause) {
            return;
        }
        self.mark_need_resched(cpu_id);
    }

    /// `cpu_should_reschedule()` 已经判断为真时，只记录请求，不再重复判断。
    pub(super) fn mark_need_resched(&mut self, cpu_id : CpuId) {
        self.cpu_states[cpu_id.raw()].need_resched = true;
        self.pending_reschedule_cpus
            .insert(cpu_id);
    }

    pub(crate) fn take_pending_reschedule_cpus(&mut self) -> CpuMask {
        let pending = self.pending_reschedule_cpus;
        self.pending_reschedule_cpus = CpuMask::EMPTY;
        pending
    }

    /// 消费当前 CPU 的重调度请求；SSIP 没有请求位时不应触发调度。
    pub(crate) fn take_need_resched(&mut self, cpu_id : CpuId) -> bool {
        let need_resched = self.cpu_states[cpu_id.raw()].need_resched;
        self.cpu_states[cpu_id.raw()].need_resched = false;
        need_resched
    }
    /// TCB_SYNC: mark_ready → Registry,vruntime 归一化 → Registry
    /// TCB → 目标 CPU ready queue 的唯一入口。
    /// 只修改 TCB 与 runqueue，不产生 reschedule/IPI 请求。
    pub(super) fn enqueue_ready_on_cpu(&mut self, task_id : TaskId, cpu_id : CpuId) {
        assert!(Some(task_id) != self.cpu_states[cpu_id.raw()].idle_task_id,
                "idle task must not be placed on a ready queue");
        assert!(self.cpu_states[cpu_id.raw()].online,
                "ready task must target an online CPU");
        assert!(self.registry
                    .get_affinity(task_id)
                    .expect("queued task must exist")
                    .contains(cpu_id),
                "ready task must target a CPU allowed by its affinity");
        // 诊断断言：若任务仍被某个核当作 current（即它还在物理运行），绝不能发布到
        // 别的核（空闲偷取会造成双核同跑/current-task 与硬件栈脱节）。延迟迁移的合法
        // 情况是 running_cpu_id 尚未清除、但该核已 `__switch` 切走、不再是其 current。
        if let Some(running_cpu) = self.registry
                                       .running_cpu_id(task_id)
        {
            if self.cpu_states[running_cpu.raw()].current_task_id() == Some(task_id) {
                assert_eq!(running_cpu,
                           cpu_id,
                           "[sched] publishing running task {} to CPU {} while it runs on CPU {}",
                           task_id,
                           cpu_id.raw(),
                           running_cpu.raw());
            }
        }
        if let Some(old_cpu_id) = self.registry
                                      .ready_cpu_id(task_id)
        {
            // 包括同 CPU 重复入队在内，先清掉旧归属，确保一个任务只在一个
            // ready queue 中出现一次。
            self.cpu_states[old_cpu_id.raw()].dequeue(task_id);
        }
        let mut snap = self.registry
                           .task_snapshot(task_id);
        if let Some(vruntime) =
            self.cpu_states[cpu_id.raw()].normalize_vruntime(snap.vruntime, snap.policy)
        {
            snap.vruntime = vruntime;
            self.registry
                .set_vruntime(task_id, vruntime);
        }
        self.registry
            .mark_ready(task_id, cpu_id);
        self.cpu_states[cpu_id.raw()].enqueue(task_id, &snap);
    }
    /// 从所有 CPU 的就绪队列摘除任务（用于 kill / discard 等跨 CPU 操作）。
    pub(super) fn dequeue_from_all_cpus(&mut self, task_id : TaskId) {
        for cpu in &mut self.cpu_states {
            cpu.dequeue(task_id);
        }
    }
    /// 到期睡眠/超时任务到就绪队列。(超时唤醒)
    pub(super) fn activate_woken_and_timeout_tasks(&mut self) {
        for task_id in &self.wait_queues
                            .woken_tasks()
        {
            self.activate_ready_task(*task_id, ReadyPlacement::LastCpu);
        }
        for (task_id, target) in &self.wait_queues
                                      .timeout_tasks()
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
            self.activate_ready_task(*task_id, ReadyPlacement::LastCpu);
        }
    }
}

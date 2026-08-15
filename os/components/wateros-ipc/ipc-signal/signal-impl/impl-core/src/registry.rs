//! 进程、线程信号状态的生命周期与投递逻辑。
//!
//! `ARCH:` 本文件只变更信号状态并产生 `SignalDispatch` / `SignalEffect`，不执行 scheduler
//! 或用户态 signal frame 副作用；调用方必须在 registry 锁外落实这些结果。

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use api_v0::*;

use crate::state::{ProcessSignalState, RealDeadlineEntry, ThreadSignalState};

/// `DATA:` 全局信号注册表：按 PID 存进程共享状态，按 WaterOS task ID 存线程私有状态。
///
/// `INVARIANT:` `threads` 中每个条目的 `pid` 都应存在于 `processes`。`real_deadlines`
/// 可包含已失效的旧项，消费时依靠 generation 与实际 deadline 过滤。
#[derive(Default)]
pub struct SignalRegistry {
    /// disposition、进程 pending 及所有进程级 timer。
    pub(super) processes : BTreeMap<usize, ProcessSignalState>,
    /// mask、线程 pending、临时等待状态和备用信号栈；key 不是 Linux tid。
    pub(super) threads : BTreeMap<usize, ThreadSignalState>,
    /// `ITIMER_REAL` 的 deadline 索引，避免每个 tick 扫描所有进程。
    pub(super) real_deadlines : BTreeMap<u128, Vec<RealDeadlineEntry>>,
}

impl SignalRegistry {
    /// 创建空注册表。
    pub const fn new() -> Self {
        Self { processes : BTreeMap::new(),
               threads : BTreeMap::new(),
               real_deadlines : BTreeMap::new() }
    }

    pub fn register_process(&mut self, pid : usize, task_id : usize, tid : usize) {
        self.processes
            .entry(pid)
            .or_insert_with(ProcessSignalState::new);
        self.threads
            .entry(task_id)
            .or_insert_with(|| ThreadSignalState::new(pid, tid, SignalSet::empty()));
    }

    pub fn has_thread(&self, task_id : usize) -> bool {
        self.threads
            .contains_key(&task_id)
    }

    /// `FLOW:` `fork` 只继承 disposition、调用线程 mask 和备用栈，不继承 pending/timer。
    pub fn fork_process(&mut self,
                        parent_task_id : usize,
                        child_pid : usize,
                        child_task_id : usize,
                        child_tid : usize)
                        -> SignalResult<()> {
        let parent_thread = *self.threads
                                 .get(&parent_task_id)
                                 .ok_or(SignalError::NoSuchTask)?;
        let parent = self.processes
                         .get(&parent_thread.pid)
                         .ok_or(SignalError::NoSuchProcess)?;
        let mut child = ProcessSignalState::new();
        child.actions = parent.actions;
        self.processes
            .insert(child_pid, child);
        self.threads
            .insert(child_task_id, {
                let mut child_thread =
                    ThreadSignalState::new(child_pid, child_tid, parent_thread.mask);
                child_thread.alternate_stack = parent_thread.alternate_stack;
                child_thread
            });
        Ok(())
    }

    /// 在同进程内注册新线程（`clone` CLONE_THREAD 路径）。
    pub fn register_thread(&mut self,
                           parent_task_id : usize,
                           task_id : usize,
                           tid : usize)
                           -> SignalResult<()> {
        let parent = *self.threads
                          .get(&parent_task_id)
                          .ok_or(SignalError::NoSuchTask)?;
        self.threads
            .insert(task_id,
                    ThreadSignalState::new(parent.pid, tid, parent.mask));
        Ok(())
    }

    /// `FLOW:` `execve` 重置用户 handler 与调用线程备用栈，清除 POSIX timer。
    ///
    /// 忽略 disposition、pending 以及 interval timer 保持不变；其余线程应由 facade 先删除。
    pub fn exec_process(&mut self, task_id : usize) -> SignalResult<()> {
        let pid = self.thread(task_id)?
                      .pid;
        self.thread_mut(task_id)?
            .alternate_stack = AlternateSignalStack::default();
        let process = self.processes
                          .get_mut(&pid)
                          .ok_or(SignalError::NoSuchProcess)?;
        for action in &mut process.actions {
            if action.has_user_handler() {
                *action = SignalAction::default_action();
            }
        }
        process.posix_timers
               .clear();
        process.next_posix_timer_id = 0;
        Ok(())
    }

    /// 移除线程表项（不级联删除空进程）。
    pub fn drop_thread(&mut self, task_id : usize) {
        self.threads
            .remove(&task_id);
    }

    /// 移除线程；若进程已无其它线程则删除进程状态。
    pub fn drop_thread_and_empty_process(&mut self, task_id : usize) {
        let Some(thread) = self.threads
                               .remove(&task_id)
        else {
            return;
        };
        if !self.threads
                .values()
                .any(|other| other.pid == thread.pid)
        {
            self.processes
                .remove(&thread.pid);
        }
    }

    /// 强制删除进程及其全部线程状态。
    pub fn drop_process(&mut self, pid : usize) {
        self.processes
            .remove(&pid);
        self.threads
            .retain(|_, thread| thread.pid != pid);
    }

    fn thread(&self, task_id : usize) -> SignalResult<&ThreadSignalState> {
        self.threads
            .get(&task_id)
            .ok_or(SignalError::NoSuchTask)
    }

    fn thread_mut(&mut self, task_id : usize) -> SignalResult<&mut ThreadSignalState> {
        self.threads
            .get_mut(&task_id)
            .ok_or(SignalError::NoSuchTask)
    }

    pub fn alternate_stack(&self, task_id : usize) -> SignalResult<AlternateSignalStack> {
        Ok(self.thread(task_id)?
               .alternate_stack)
    }

    pub fn replace_alternate_stack(&mut self,
                                   task_id : usize,
                                   replacement : AlternateSignalStack)
                                   -> SignalResult<AlternateSignalStack> {
        let thread = self.thread_mut(task_id)?;
        if thread.alternate_stack
                 .active_frames !=
           0
        {
            return Err(SignalError::AlternateStackActive);
        }
        let old = thread.alternate_stack;
        thread.alternate_stack = replacement;
        Ok(old)
    }

    pub fn enter_signal_frame(&mut self,
                              task_id : usize,
                              on_alternate_stack : bool)
                              -> SignalResult<()> {
        if on_alternate_stack {
            let stack = &mut self.thread_mut(task_id)?
                                 .alternate_stack;
            stack.active_frames = stack.active_frames
                                       .saturating_add(1);
        }
        Ok(())
    }

    pub fn leave_signal_frame(&mut self,
                              task_id : usize,
                              on_alternate_stack : bool)
                              -> SignalResult<()> {
        if on_alternate_stack {
            let stack = &mut self.thread_mut(task_id)?
                                 .alternate_stack;
            stack.active_frames = stack.active_frames
                                       .saturating_sub(1);
        }
        Ok(())
    }

    /// 查询进程级 disposition（`rt_sigaction` GET）。
    pub fn get_action(&self, task_id : usize, sig : usize) -> SignalResult<SignalAction> {
        if !valid_signal(sig) {
            return Err(SignalError::InvalidSignal);
        }
        let pid = self.thread(task_id)?
                      .pid;
        Ok(self.processes
               .get(&pid)
               .ok_or(SignalError::NoSuchProcess)?
               .action(sig))
    }

    /// 设置进程级 disposition（`rt_sigaction` SET）；ignore 时清除相关 pending。
    pub fn set_action(&mut self,
                      task_id : usize,
                      sig : usize,
                      mut action : SignalAction)
                      -> SignalResult<SignalAction> {
        if !valid_signal(sig) || immutable_signal(sig) {
            return Err(SignalError::InvalidSignal);
        }
        let pid = self.thread(task_id)?
                      .pid;
        action.mask.remove(SIGKILL);
        action.mask.remove(SIGSTOP);
        let process = self.processes
                          .get_mut(&pid)
                          .ok_or(SignalError::NoSuchProcess)?;
        let old = process.actions[sig - 1];
        process.actions[sig - 1] = action;
        if action.is_ignore() {
            process.pending
                   .remove(sig);
            for thread in self.threads
                              .values_mut()
                              .filter(|thread| thread.pid == pid)
            {
                thread.pending
                      .remove(sig);
            }
        }
        Ok(old)
    }

    #[cfg(test)]
    fn current_mask(&self, task_id : usize) -> SignalResult<SignalSet> {
        Ok(self.thread(task_id)?
               .mask)
    }

    /// 直接替换线程阻塞掩码（`SIG_SETMASK` 语义）。
    pub fn replace_mask(&mut self, task_id : usize, mut mask : SignalSet) -> SignalResult<()> {
        mask.remove(SIGKILL);
        mask.remove(SIGSTOP);
        self.thread_mut(task_id)?
            .mask = mask;
        Ok(())
    }

    /// `sigsuspend` 进入：保存原掩码并安装临时掩码。
    pub fn begin_sigsuspend(&mut self,
                            task_id : usize,
                            mut temporary_mask : SignalSet)
                            -> SignalResult<()> {
        temporary_mask.remove(SIGKILL);
        temporary_mask.remove(SIGSTOP);
        let thread = self.thread_mut(task_id)?;
        if thread.temporary_restore_mask
                 .is_some()
        {
            return Err(SignalError::InvalidHow);
        }
        thread.temporary_restore_mask = Some(thread.mask);
        thread.mask = temporary_mask;
        Ok(())
    }

    /// `sigsuspend` 退出：恢复原掩码。
    pub fn end_sigsuspend(&mut self, task_id : usize) -> SignalResult<()> {
        let thread = self.thread_mut(task_id)?;
        if let Some(restore) = thread.temporary_restore_mask
                                     .take()
        {
            thread.mask = restore;
        }
        Ok(())
    }

    /// `ppoll` / `pselect6`：在阻塞等待期间临时替换线程信号掩码。
    pub fn begin_poll_sigmask(&mut self,
                              task_id : usize,
                              mut temporary_mask : SignalSet)
                              -> SignalResult<()> {
        temporary_mask.remove(SIGKILL);
        temporary_mask.remove(SIGSTOP);
        let thread = self.thread_mut(task_id)?;
        if thread.temporary_restore_mask
                 .is_some()
        {
            return Err(SignalError::InvalidHow);
        }
        thread.temporary_restore_mask = Some(thread.mask);
        thread.mask = temporary_mask;
        Ok(())
    }

    pub fn end_poll_sigmask(&mut self, task_id : usize) -> SignalResult<()> {
        let thread = self.thread_mut(task_id)?;
        if let Some(restore) = thread.temporary_restore_mask
                                     .take()
        {
            thread.mask = restore;
        }
        Ok(())
    }

    /// `sigwait` 进入：登记等待信号集。
    pub fn begin_signal_wait(&mut self, task_id : usize, wait_set : SignalSet) -> SignalResult<()> {
        self.thread_mut(task_id)?
            .waiting_for = Some(wait_set);
        Ok(())
    }

    /// `sigwait` 退出：清除等待集。
    pub fn end_signal_wait(&mut self, task_id : usize) -> SignalResult<()> {
        self.thread_mut(task_id)?
            .waiting_for = None;
        Ok(())
    }

    /// 按 `how` 更新线程阻塞掩码（`rt_sigprocmask`）。
    pub fn update_mask(&mut self,
                       task_id : usize,
                       how : usize,
                       set : Option<SignalSet>)
                       -> SignalResult<SignalSet> {
        let thread = self.thread_mut(task_id)?;
        let old = thread.mask;
        let Some(mut set) = set else {
            return Ok(old);
        };
        set.remove(SIGKILL);
        set.remove(SIGSTOP);
        thread.mask = match how {
            SIG_BLOCK => thread.mask
                               .union(set),
            SIG_UNBLOCK => thread.mask
                                 .difference(set),
            SIG_SETMASK => set,
            _ => return Err(SignalError::InvalidHow),
        };
        Ok(old)
    }

    fn generation_delivery(&self, pid : usize, sig : usize) -> SignalResult<SignalDelivery> {
        if !valid_signal(sig) {
            return Err(SignalError::InvalidSignal);
        }
        if sig == SIGSTOP {
            return Ok(SignalDelivery::Stop);
        }
        let action = self.processes
                         .get(&pid)
                         .ok_or(SignalError::NoSuchProcess)?
                         .action(sig);
        // Default-ignored signals (SIGCHLD/SIGURG/SIGWINCH) must still become
        // pending when blocked and consumed through signalfd(2)/sigwait(2).
        // Only an explicit SIG_IGN disposition can discard the signal at
        // generation time; the normal default-ignore path is handled again
        // when a thread later selects the pending signal for delivery.
        if action.is_ignore() {
            Ok(SignalDelivery::Ignored)
        } else if sig == SIGCONT {
            Ok(SignalDelivery::Continue)
        } else {
            Ok(SignalDelivery::Pending)
        }
    }

    /// `FLOW:` 向指定线程投递信号（`tkill` / `pthread_kill` 路径）。
    ///
    /// 线程定向信号只进入 `ThreadSignalState::pending`。结果中的 target 仅供锁外唤醒，
    /// 不是已经切换到该线程。
    pub fn send_thread(&mut self, task_id : usize, sig : usize) -> SignalResult<SignalDispatch> {
        let pid = self.thread(task_id)?
                      .pid;
        match self.generation_delivery(pid, sig)? {
            SignalDelivery::Ignored => Ok(SignalDispatch::ignored()),
            SignalDelivery::Continue => {
                let action = self.processes
                                 .get(&pid)
                                 .ok_or(SignalError::NoSuchProcess)?
                                 .action(sig);
                if action.has_user_handler() {
                    self.thread_mut(task_id)?
                        .pending
                        .insert(sig);
                }
                Ok(SignalDispatch::continued(Some(task_id)))
            }
            SignalDelivery::Stop => Ok(SignalDispatch::stop(Some(task_id))),
            SignalDelivery::Pending => {
                let thread = self.thread_mut(task_id)?;
                thread
                    .pending
                    .insert(sig);
                let should_wake = !thread.mask.contains(sig) ||
                                  thread.waiting_for
                                        .is_some_and(|set| set.contains(sig));
                Ok(SignalDispatch::pending(should_wake.then_some(task_id)))
            }
        }
    }

    /// `FLOW:` 向进程投递信号（`kill` 路径）；选择最低未屏蔽 tid 或 `sigwait` 线程。
    ///
    /// 普通信号记录在进程共享 pending 中；选择的 task ID 只决定优先唤醒谁，真正消费者在
    /// 自己的安全点通过 [`Self::take_deliverable`] 决定。
    pub fn send_process(&mut self, pid : usize, sig : usize) -> SignalResult<SignalDispatch> {
        let delivery = self.generation_delivery(pid, sig)?;
        if delivery == SignalDelivery::Ignored {
            return Ok(SignalDispatch::ignored());
        }
        let target = self.threads
                         .iter()
                         .filter(|(_, thread)| {
                             thread.pid == pid &&
                             (matches!(delivery,
                                       SignalDelivery::Stop | SignalDelivery::Continue) ||
                              !thread.mask
                                     .contains(sig) ||
                              thread.waiting_for
                                    .is_some_and(|set| set.contains(sig)))
                         })
                         .min_by_key(|(_, thread)| thread.tid)
                         .map(|(task_id, _)| *task_id);
        if delivery == SignalDelivery::Continue {
            let action = self.processes
                             .get(&pid)
                             .ok_or(SignalError::NoSuchProcess)?
                             .action(sig);
            if action.has_user_handler() {
                self.processes
                    .get_mut(&pid)
                    .ok_or(SignalError::NoSuchProcess)?
                    .pending
                    .insert(sig);
            }
            return Ok(SignalDispatch::continued(target));
        }
        if delivery == SignalDelivery::Stop {
            return Ok(SignalDispatch::stop(target));
        }
        self.processes
            .get_mut(&pid)
            .ok_or(SignalError::NoSuchProcess)?
            .pending
            .insert(sig);
        let wake_target = self.threads
                              .iter()
                              .filter(|(_, thread)| thread.pid == pid)
                              .find_map(|(task_id, _)| {
                                  self.has_deliverable(*task_id)
                                      .ok()
                                      .filter(|v| *v)
                                      .map(|_| *task_id)
                              })
                              .or(target);
        Ok(SignalDispatch::pending(wake_target))
    }

    /// 返回线程 pending 与进程 pending 的并集。
    pub fn pending(&self, task_id : usize) -> SignalResult<SignalSet> {
        let thread = self.thread(task_id)?;
        let process = self.processes
                          .get(&thread.pid)
                          .ok_or(SignalError::NoSuchProcess)?;
        Ok(thread.pending
                 .union(process.pending))
    }

    /// 是否存在未被掩码阻塞的可交付信号。
    pub fn has_deliverable(&self, task_id : usize) -> SignalResult<bool> {
        let thread = self.thread(task_id)?;
        let process = self.processes
                          .get(&thread.pid)
                          .ok_or(SignalError::NoSuchProcess)?;
        Ok(!thread.pending
                  .union(process.pending)
                  .difference(thread.mask)
                  .is_empty())
    }

    /// 停止态线程仍必须能消费 SIGKILL；其它 pending 保持到 SIGCONT 之后。
    pub fn take_sigkill(&mut self, task_id : usize) -> bool {
        let Some(thread) = self.threads
                               .get(&task_id)
                               .copied()
        else {
            return false;
        };
        if self.threads
               .get(&task_id)
               .is_some_and(|thread| thread.pending.contains(SIGKILL))
        {
            self.threads
                .get_mut(&task_id)
                .expect("thread checked above")
                .pending
                .remove(SIGKILL);
        } else {
            let Some(process) = self.processes
                                    .get_mut(&thread.pid)
            else {
                return false;
            };
            if !process.pending
                       .contains(SIGKILL)
            {
                return false;
            }
            process.pending
                   .remove(SIGKILL);
        }
        if let Some(thread) = self.threads
                                  .get_mut(&task_id)
        {
            thread.temporary_restore_mask = None;
            thread.waiting_for = None;
        }
        true
    }

    /// 从 pending 中取出 `wait_set` 内第一个信号（`sigwait`）。
    pub fn take_pending(&mut self, task_id : usize, wait_set : SignalSet) -> Option<usize> {
        self.take_pending_record(task_id, wait_set)
            .map(|record| record.signal)
    }

    /// 取出一个 pending 信号并保留其线程/进程归属，供可回滚读取使用。
    pub fn take_pending_record(&mut self,
                               task_id : usize,
                               wait_set : SignalSet)
                               -> Option<TakenPendingSignal> {
        let thread = *self.threads
                          .get(&task_id)?;
        let thread_ready = thread.pending
                                 .intersection(wait_set);
        if let Some(sig) = thread_ready.first_signal() {
            self.threads
                .get_mut(&task_id)?
                .pending
                .remove(sig);
            return Some(TakenPendingSignal { signal : sig,
                                             scope : PendingSignalScope::Thread });
        }
        let process = self.processes
                          .get_mut(&thread.pid)?;
        let sig = process.pending
                         .intersection(wait_set)
                         .first_signal()?;
        process.pending
               .remove(sig);
        Some(TakenPendingSignal { signal : sig,
                                  scope : PendingSignalScope::Process })
    }

    /// 将尚未成功交付到用户空间的 pending 记录放回原集合。
    pub fn restore_pending_record(&mut self,
                                  task_id : usize,
                                  record : TakenPendingSignal)
                                  -> SignalResult<()> {
        let pid = self.thread(task_id)?.pid;
        match record.scope {
            PendingSignalScope::Thread => self.thread_mut(task_id)?.pending.insert(record.signal),
            PendingSignalScope::Process => self.processes
                                                       .get_mut(&pid)
                                                       .ok_or(SignalError::NoSuchProcess)?
                                                       .pending
                                                       .insert(record.signal),
        }
        Ok(())
    }

    /// `FLOW:` 在目标线程安全点取出最低编号可交付信号，并按**当前** disposition 决定效果。
    ///
    /// `INVARIANT:` 初次投递到最终交付之间，mask 与 `sigaction` 可能改变，所以不能在
    /// `send_*` 时提前固定 handler/终止语义。忽略信号会递归跳过，直到得到有效效果或无信号。
    pub fn take_deliverable(&mut self, task_id : usize) -> Option<SignalEffect> {
        let thread = *self.threads
                          .get(&task_id)?;
        let process = self.processes
                          .get(&thread.pid)?;
        let deliverable = thread.pending
                                .union(process.pending)
                                .difference(thread.mask);
        let sig = deliverable.first_signal()?;
        let action = process.action(sig);
        let scope = if self.threads
                           .get(&task_id)
                           .is_some_and(|thread| {
                               thread.pending
                                     .contains(sig)
                           })
        {
            self.threads
                .get_mut(&task_id)?
                .pending
                .remove(sig);
            PendingSignalScope::Thread
        } else {
            self.processes
                .get_mut(&thread.pid)?
                .pending
                .remove(sig);
            PendingSignalScope::Process
        };
        let delivery_mask = thread.mask;
        let previous_mask = thread.temporary_restore_mask
                                  .unwrap_or(delivery_mask);
        if sig == SIGKILL || action.is_default() && default_terminates(sig) {
            let target_thread = self.threads
                                    .get_mut(&task_id)?;
            target_thread.temporary_restore_mask = None;
            target_thread.waiting_for = None;
            return Some(SignalEffect::Terminate { signal : sig });
        }
        if action.is_default() && default_stops(sig) {
            let target_thread = self.threads
                                    .get_mut(&task_id)?;
            target_thread.mask = previous_mask;
            target_thread.temporary_restore_mask = None;
            target_thread.waiting_for = None;
            return Some(SignalEffect::Stop { signal : sig });
        }
        if sig == SIGCONT && !action.has_user_handler() {
            let target_thread = self.threads
                                    .get_mut(&task_id)?;
            target_thread.mask = previous_mask;
            target_thread.temporary_restore_mask = None;
            target_thread.waiting_for = None;
            return Some(SignalEffect::Continue { signal : sig });
        }
        if action.is_ignore() || action.is_default() {
            let next = self.take_deliverable(task_id);
            if next.is_none() {
                let _ = self.end_sigsuspend(task_id);
            }
            return next;
        }
        let mut handler_mask = delivery_mask.union(action.mask);
        if action.flags & SA_NODEFER == 0 {
            handler_mask.insert(sig);
        }
        let target_thread = self.threads
                                .get_mut(&task_id)?;
        target_thread.mask = handler_mask;
        target_thread.temporary_restore_mask = None;
        target_thread.waiting_for = None;
        if action.flags & SA_RESETHAND != 0 {
            self.processes
                .get_mut(&thread.pid)?
                .actions[sig - 1] = SignalAction::default_action();
        }
        Some(SignalEffect::Handler(PendingSignal { signal : sig,
                                                   scope,
                                                   action,
                                                   previous_mask }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry_with_process() -> SignalRegistry {
        let mut registry = SignalRegistry::new();
        registry.register_process(10, 100, 10);
        registry
    }

    #[test]
    fn realtime_timer_replaces_disables_and_advances_without_drift() {
        let mut registry = registry_with_process();
        registry.set_action(100,
                            SIGALRM,
                            SignalAction { handler : 0x1000,
                                           ..SignalAction::default_action() })
                .unwrap();
        let first = IntervalTimerSpec { interval_ns : 10,
                                        value_ns : 20 };
        assert_eq!(registry.set_timer(10, ITIMER_REAL, first, 100)
                           .unwrap(),
                   IntervalTimerSpec::default());
        assert_eq!(registry.get_timer(10, ITIMER_REAL, 105)
                           .unwrap()
                           .value_ns,
                   15);

        let dispatches = registry.expire_realtime(145);
        assert_eq!(dispatches.len(), 1);
        assert_eq!(registry.get_timer(10, ITIMER_REAL, 145)
                           .unwrap()
                           .value_ns,
                   5);
        assert_eq!(registry.pending(100)
                           .unwrap()
                           .bits(),
                   SignalSet::from_bits(1 << (SIGALRM - 1)).bits());

        let old = registry.set_timer(10,
                                     ITIMER_REAL,
                                     IntervalTimerSpec::default(),
                                     146)
                          .unwrap();
        assert_eq!(old.value_ns, 4);
        assert_eq!(registry.get_timer(10, ITIMER_REAL, 1_000)
                           .unwrap()
                           .value_ns,
                   0);
    }

    #[test]
    fn standard_pending_coalesces_and_targets_lowest_unmasked_tid() {
        let mut registry = registry_with_process();
        registry.register_thread(100, 101, 11)
                .unwrap();
        registry.replace_mask(100,
                              SignalSet::from_bits(1 << (SIGALRM - 1)))
                .unwrap();
        let action = SignalAction { handler : 0x1000,
                                    ..SignalAction::default_action() };
        registry.set_action(100, SIGALRM, action)
                .unwrap();

        let first = registry.send_process(10, SIGALRM)
                            .unwrap();
        let second = registry.send_process(10, SIGALRM)
                             .unwrap();
        assert_eq!(first.target_task_id, Some(101));
        assert_eq!(second.target_task_id, Some(101));
        assert_eq!(registry.pending(101)
                           .unwrap()
                           .bits()
                           .count_ones(),
                   1);
    }

    #[test]
    fn handler_effect_preserves_thread_or_process_pending_scope() {
        let mut registry = registry_with_process();
        let action = SignalAction { handler : 0x1000,
                                    ..SignalAction::default_action() };
        registry.set_action(100, SIGILL, action)
                .unwrap();
        registry.set_action(100, SIGSEGV, action)
                .unwrap();

        registry.send_thread(100, SIGILL)
                .unwrap();
        let thread_effect = registry.take_deliverable(100);
        assert!(matches!(thread_effect,
                         Some(SignalEffect::Handler(PendingSignal {
                             signal: SIGILL,
                             scope: PendingSignalScope::Thread,
                             ..
                         }))));

        registry.send_process(10, SIGSEGV)
                .unwrap();
        let process_effect = registry.take_deliverable(100);
        assert!(matches!(process_effect,
                         Some(SignalEffect::Handler(PendingSignal {
                             signal: SIGSEGV,
                             scope: PendingSignalScope::Process,
                             ..
                         }))));
    }

    #[test]
    fn fork_copies_dispositions_and_mask_but_not_timer_or_pending() {
        let mut registry = registry_with_process();
        let action = SignalAction { handler : 0x2000,
                                    flags : SA_RESTART,
                                    ..SignalAction::default_action() };
        registry.set_action(100, SIGUSR1, action)
                .unwrap();
        registry.replace_mask(100,
                              SignalSet::from_bits(1 << (SIGUSR2 - 1)))
                .unwrap();
        registry.set_timer(10,
                           ITIMER_REAL,
                           IntervalTimerSpec { interval_ns : 0,
                                               value_ns : 50 },
                           100)
                .unwrap();
        registry.send_process(10, SIGUSR1)
                .unwrap();

        registry.fork_process(100, 20, 200, 20)
                .unwrap();
        assert_eq!(registry.get_action(200, SIGUSR1)
                           .unwrap(),
                   action);
        assert!(registry.current_mask(200)
                        .unwrap()
                        .contains(SIGUSR2));
        assert_eq!(registry.get_timer(20, ITIMER_REAL, 100)
                           .unwrap(),
                   IntervalTimerSpec::default());
        assert!(registry.pending(200)
                        .unwrap()
                        .is_empty());
    }

    #[test]
    fn cpu_timers_use_distinct_user_and_total_clocks() {
        let mut registry = registry_with_process();
        for (signal, handler) in [(SIGVTALRM, 0x3000),
                                  (SIGPROF, 0x4000)]
        {
            registry.set_action(100,
                                signal,
                                SignalAction { handler,
                                               ..SignalAction::default_action() })
                    .unwrap();
        }
        registry.set_timer(10,
                           ITIMER_VIRTUAL,
                           IntervalTimerSpec { interval_ns : 0,
                                               value_ns : 10 },
                           0)
                .unwrap();
        registry.set_timer(10,
                           ITIMER_PROF,
                           IntervalTimerSpec { interval_ns : 0,
                                               value_ns : 15 },
                           0)
                .unwrap();

        assert!(registry.account_cpu(10, 9, 9)
                        .unwrap()
                        .is_empty());
        let expired = registry.account_cpu(10, 1, 6)
                              .unwrap();
        assert_eq!(expired.len(), 2);
        assert!(registry.pending(100)
                        .unwrap()
                        .contains(SIGVTALRM));
        assert!(registry.pending(100)
                        .unwrap()
                        .contains(SIGPROF));
    }

    #[test]
    fn realtime_generation_discards_stale_deadline_entries() {
        let mut registry = registry_with_process();
        registry.set_action(100,
                            SIGALRM,
                            SignalAction { handler : 0x5000,
                                           ..SignalAction::default_action() })
                .unwrap();
        registry.set_timer(10,
                           ITIMER_REAL,
                           IntervalTimerSpec { interval_ns : 0,
                                               value_ns : 20 },
                           100)
                .unwrap();
        registry.set_timer(10,
                           ITIMER_REAL,
                           IntervalTimerSpec { interval_ns : 0,
                                               value_ns : 40 },
                           100)
                .unwrap();

        assert!(registry.expire_realtime(120)
                        .is_empty());
        assert_eq!(registry.expire_realtime(140)
                           .len(),
                   1);
    }

    #[test]
    fn exec_preserves_ignore_pending_and_timer_but_resets_caught_handler() {
        let mut registry = registry_with_process();
        registry.set_action(100, SIGUSR1, SignalAction::ignore())
                .unwrap();
        registry.set_action(100,
                            SIGUSR2,
                            SignalAction { handler : 0x6000,
                                           ..SignalAction::default_action() })
                .unwrap();
        registry.set_timer(10,
                           ITIMER_REAL,
                           IntervalTimerSpec { interval_ns : 5,
                                               value_ns : 20 },
                           100)
                .unwrap();
        registry.send_thread(100, SIGUSR2)
                .unwrap();

        registry.exec_process(100)
                .unwrap();

        assert!(registry.get_action(100, SIGUSR1)
                        .unwrap()
                        .is_ignore());
        assert!(registry.get_action(100, SIGUSR2)
                        .unwrap()
                        .is_default());
        assert!(registry.pending(100)
                        .unwrap()
                        .contains(SIGUSR2));
        assert_eq!(registry.get_timer(10, ITIMER_REAL, 105)
                           .unwrap()
                           .value_ns,
                   15);
    }

    #[test]
    fn sigsuspend_restores_original_mask_through_signal_frame() {
        let mut registry = registry_with_process();
        registry.set_action(100,
                            SIGUSR1,
                            SignalAction { handler : 0x7000,
                                           ..SignalAction::default_action() })
                .unwrap();
        let original = SignalSet::from_bits(1 << (SIGUSR2 - 1));
        registry.replace_mask(100, original)
                .unwrap();
        registry.begin_sigsuspend(100, SignalSet::empty())
                .unwrap();
        registry.send_thread(100, SIGUSR1)
                .unwrap();

        let SignalEffect::Handler(pending) = registry.take_deliverable(100)
                                                      .unwrap()
        else {
            panic!("expected handler delivery");
        };
        assert_eq!(pending.previous_mask, original);
        let handler_mask = registry.current_mask(100)
                                   .unwrap();
        assert!(handler_mask.contains(SIGUSR1));
        assert!(!handler_mask.contains(SIGUSR2));
        registry.replace_mask(100, pending.previous_mask)
                .unwrap();
        assert_eq!(registry.current_mask(100)
                           .unwrap(),
                   original);
        registry.begin_sigsuspend(100, SignalSet::empty())
                .expect("delivery must clear the previous sigsuspend state");
        registry.end_sigsuspend(100)
                .unwrap();
    }

    #[test]
    fn blocked_default_terminate_remains_pending_until_unblocked() {
        let mut registry = registry_with_process();
        let mut blocked = SignalSet::empty();
        blocked.insert(SIGTERM);
        registry.replace_mask(100, blocked)
                .unwrap();

        let dispatch = registry.send_thread(100, SIGTERM)
                               .unwrap();
        assert_eq!(dispatch.delivery, SignalDelivery::Pending);
        assert_eq!(dispatch.target_task_id, None);
        assert!(registry.pending(100)
                        .unwrap()
                        .contains(SIGTERM));
        assert_eq!(registry.take_deliverable(100), None);

        registry.replace_mask(100, SignalSet::empty())
                .unwrap();
        assert_eq!(registry.take_deliverable(100),
                   Some(SignalEffect::Terminate { signal : SIGTERM }));
    }

    #[test]
    fn sigkill_can_be_consumed_while_other_signals_remain_pending() {
        let mut registry = registry_with_process();
        registry.send_process(10, SIGKILL)
                .unwrap();
        registry.send_process(10, SIGTERM)
                .unwrap();

        assert!(registry.take_sigkill(100));
        assert!(!registry.take_sigkill(100));
        assert!(registry.pending(100)
                        .unwrap()
                        .contains(SIGTERM));
    }

    #[test]
    fn stop_signals_are_decided_at_delivery_and_sigstop_is_immutable() {
        let mut registry = registry_with_process();
        assert_eq!(registry.set_action(100, SIGSTOP, SignalAction::ignore()),
                   Err(SignalError::InvalidSignal));
        assert_eq!(registry.send_thread(100, SIGSTOP)
                           .unwrap()
                           .delivery,
                   SignalDelivery::Stop);
        assert!(!registry.pending(100)
                         .unwrap()
                         .contains(SIGSTOP));

        registry.send_thread(100, SIGTSTP)
                .unwrap();
        assert_eq!(registry.take_deliverable(100),
                   Some(SignalEffect::Stop { signal : SIGTSTP }));
    }

    #[test]
    fn signal_64_is_valid_and_caught_sigcont_is_both_continue_and_pending() {
        let mut registry = registry_with_process();
        let handler = SignalAction { handler : 0x8000,
                                     ..SignalAction::default_action() };
        registry.set_action(100, NSIG, handler)
                .unwrap();
        registry.send_thread(100, NSIG)
                .unwrap();
        assert!(matches!(registry.take_deliverable(100),
                         Some(SignalEffect::Handler(PendingSignal {
                             signal: NSIG,
                             ..
                         }))));

        registry.set_action(100, SIGCONT, handler)
                .unwrap();
        let dispatch = registry.send_process(10, SIGCONT)
                               .unwrap();
        assert_eq!(dispatch.delivery, SignalDelivery::Continue);
        assert!(matches!(registry.take_deliverable(100),
                         Some(SignalEffect::Handler(PendingSignal {
                             signal: SIGCONT,
                             ..
                         }))));
    }

    #[test]
    fn alternate_stack_follows_fork_clone_and_exec_rules() {
        let mut registry = registry_with_process();
        let stack = AlternateSignalStack { sp : 0x8000,
                                           size : 0x4000,
                                           active_frames : 0 };
        registry.replace_alternate_stack(100, stack)
                .unwrap();

        registry.fork_process(100, 20, 200, 20)
                .unwrap();
        assert_eq!(registry.alternate_stack(200)
                           .unwrap(),
                   stack);

        registry.register_thread(100, 101, 11)
                .unwrap();
        assert_eq!(registry.alternate_stack(101)
                           .unwrap(),
                   AlternateSignalStack::default());

        registry.exec_process(100)
                .unwrap();
        assert_eq!(registry.alternate_stack(100)
                           .unwrap(),
                   AlternateSignalStack::default());
    }

    #[test]
    fn active_alternate_stack_cannot_be_replaced() {
        let mut registry = registry_with_process();
        let stack = AlternateSignalStack { sp : 0x8000,
                                           size : 0x4000,
                                           active_frames : 0 };
        registry.replace_alternate_stack(100, stack)
                .unwrap();
        registry.enter_signal_frame(100, true)
                .unwrap();
        assert_eq!(registry.replace_alternate_stack(100, AlternateSignalStack::default()),
                   Err(SignalError::AlternateStackActive));
        registry.leave_signal_frame(100, true)
                .unwrap();
        assert_eq!(registry.replace_alternate_stack(100, AlternateSignalStack::default()),
                   Ok(stack));
    }

    #[test]
    fn posix_timer_reloads_reports_overrun_and_deletes() {
        let mut registry = registry_with_process();
        registry.set_action(100,
                            SIGALRM,
                            SignalAction { handler : 0x9000,
                                           ..SignalAction::default_action() })
                .unwrap();
        let timer_id = registry.create_posix_timer(10, PosixTimerClock::Monotonic, SIGALRM)
                               .unwrap();
        registry.set_posix_timer(10,
                                 timer_id,
                                 IntervalTimerSpec { interval_ns : 10,
                                                     value_ns : 20 },
                                 100,
                                 1_000,
                                 false)
                .unwrap();
        assert_eq!(registry.get_posix_timer(10, timer_id, 105, 1_005)
                           .unwrap()
                           .value_ns,
                   15);
        assert_eq!(registry.expire_posix_timers(145, 1_045)
                           .len(),
                   1);
        assert_eq!(registry.get_posix_timer_overrun(10, timer_id)
                           .unwrap(),
                   2);
        assert_eq!(registry.get_posix_timer(10, timer_id, 145, 1_045)
                           .unwrap()
                           .value_ns,
                   5);
        registry.delete_posix_timer(10, timer_id)
                .unwrap();
        assert_eq!(registry.get_posix_timer(10, timer_id, 145, 1_045),
                   Err(SignalError::NoSuchTimer));
    }

    #[test]
    fn posix_timers_are_not_inherited_and_are_removed_on_exec() {
        let mut registry = registry_with_process();
        let timer_id = registry.create_posix_timer(10, PosixTimerClock::Realtime, SIGALRM)
                               .unwrap();
        registry.fork_process(100, 20, 200, 20)
                .unwrap();
        assert_eq!(registry.get_posix_timer(20, timer_id, 0, 0),
                   Err(SignalError::NoSuchTimer));
        registry.exec_process(100)
                .unwrap();
        assert_eq!(registry.get_posix_timer(10, timer_id, 0, 0),
                   Err(SignalError::NoSuchTimer));
    }
}

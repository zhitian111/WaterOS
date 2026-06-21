#![no_std]
//! Process-shared signal dispositions/pending state and thread-local masks.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;

pub mod api {
    pub use api_v0::*;
}

pub use api_v0::*;

#[derive(Clone, Copy, Debug, Default)]
struct IntervalTimerState {
    interval_ns : u128,
    deadline_ns : Option<u128>,
    generation : u64,
}

impl IntervalTimerState {
    fn remaining(self, now_ns : u128) -> IntervalTimerSpec {
        IntervalTimerSpec { interval_ns : self.interval_ns,
                            value_ns : self.deadline_ns
                                           .map(|deadline| deadline.saturating_sub(now_ns))
                                           .unwrap_or(0) }
    }

    fn replace(&mut self, spec : IntervalTimerSpec, now_ns : u128) {
        self.interval_ns = spec.interval_ns;
        self.deadline_ns = (spec.value_ns != 0).then(|| now_ns.saturating_add(spec.value_ns));
        self.generation = self.generation
                              .wrapping_add(1);
    }

    fn expire(&mut self, now_ns : u128) -> bool {
        let Some(deadline) = self.deadline_ns else {
            return false;
        };
        if deadline > now_ns {
            return false;
        }
        if self.interval_ns == 0 {
            self.deadline_ns = None;
        } else {
            let overdue = now_ns.saturating_sub(deadline);
            let periods = overdue / self.interval_ns + 1;
            self.deadline_ns =
                Some(deadline.saturating_add(periods.saturating_mul(self.interval_ns)));
        }
        true
    }
}

#[derive(Clone, Debug)]
struct ProcessSignalState {
    actions : [SignalAction; NSIG],
    pending : SignalSet,
    real : IntervalTimerState,
    virtual_timer : IntervalTimerState,
    prof : IntervalTimerState,
    user_cpu_ns : u128,
    total_cpu_ns : u128,
}

impl ProcessSignalState {
    fn new() -> Self {
        Self { actions : [SignalAction::default_action(); NSIG],
               pending : SignalSet::empty(),
               real : IntervalTimerState::default(),
               virtual_timer : IntervalTimerState::default(),
               prof : IntervalTimerState::default(),
               user_cpu_ns : 0,
               total_cpu_ns : 0 }
    }

    fn action(&self, sig : usize) -> SignalAction { self.actions[sig] }

    fn timer(&self, which : usize) -> SignalResult<&IntervalTimerState> {
        match which {
            ITIMER_REAL => Ok(&self.real),
            ITIMER_VIRTUAL => Ok(&self.virtual_timer),
            ITIMER_PROF => Ok(&self.prof),
            _ => Err(SignalError::InvalidTimer),
        }
    }

    fn timer_mut(&mut self, which : usize) -> SignalResult<&mut IntervalTimerState> {
        match which {
            ITIMER_REAL => Ok(&mut self.real),
            ITIMER_VIRTUAL => Ok(&mut self.virtual_timer),
            ITIMER_PROF => Ok(&mut self.prof),
            _ => Err(SignalError::InvalidTimer),
        }
    }

    fn timer_clock(&self, which : usize, monotonic_ns : u128) -> SignalResult<u128> {
        match which {
            ITIMER_REAL => Ok(monotonic_ns),
            ITIMER_VIRTUAL => Ok(self.user_cpu_ns),
            ITIMER_PROF => Ok(self.total_cpu_ns),
            _ => Err(SignalError::InvalidTimer),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct ThreadSignalState {
    pid : usize,
    tid : usize,
    mask : SignalSet,
    pending : SignalSet,
    suspend_restore_mask : Option<SignalSet>,
    waiting_for : Option<SignalSet>,
}

impl ThreadSignalState {
    fn new(pid : usize, tid : usize, inherited_mask : SignalSet) -> Self {
        Self { pid,
               tid,
               mask : inherited_mask,
               pending : SignalSet::empty(),
               suspend_restore_mask : None,
               waiting_for : None }
    }
}

#[derive(Clone, Copy, Debug)]
struct RealDeadlineEntry {
    pid : usize,
    generation : u64,
}

#[derive(Default)]
pub struct SignalRegistry {
    processes : BTreeMap<usize, ProcessSignalState>,
    threads : BTreeMap<usize, ThreadSignalState>,
    real_deadlines : BTreeMap<u128, Vec<RealDeadlineEntry>>,
}

impl SignalRegistry {
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

    pub fn has_process(&self, pid : usize) -> bool {
        self.processes
            .contains_key(&pid)
    }

    pub fn has_thread(&self, task_id : usize) -> bool {
        self.threads
            .contains_key(&task_id)
    }

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
            .insert(child_task_id,
                    ThreadSignalState::new(child_pid, child_tid, parent_thread.mask));
        Ok(())
    }

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

    pub fn exec_process(&mut self, task_id : usize) -> SignalResult<()> {
        let pid = self.thread(task_id)?
                      .pid;
        let process = self.processes
                          .get_mut(&pid)
                          .ok_or(SignalError::NoSuchProcess)?;
        for action in &mut process.actions {
            if action.has_user_handler() {
                *action = SignalAction::default_action();
            }
        }
        Ok(())
    }

    pub fn drop_thread(&mut self, task_id : usize) {
        self.threads
            .remove(&task_id);
    }

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

    pub fn set_action(&mut self,
                      task_id : usize,
                      sig : usize,
                      action : SignalAction)
                      -> SignalResult<SignalAction> {
        if !valid_signal(sig) || immutable_signal(sig) {
            return Err(SignalError::InvalidSignal);
        }
        let pid = self.thread(task_id)?
                      .pid;
        let process = self.processes
                          .get_mut(&pid)
                          .ok_or(SignalError::NoSuchProcess)?;
        let old = process.actions[sig];
        process.actions[sig] = action;
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

    pub fn current_mask(&self, task_id : usize) -> SignalResult<SignalSet> {
        Ok(self.thread(task_id)?
               .mask)
    }

    pub fn replace_mask(&mut self, task_id : usize, mut mask : SignalSet) -> SignalResult<()> {
        mask.remove(SIGKILL);
        mask.remove(SIGSTOP);
        self.thread_mut(task_id)?
            .mask = mask;
        Ok(())
    }

    pub fn begin_sigsuspend(&mut self,
                            task_id : usize,
                            mut temporary_mask : SignalSet)
                            -> SignalResult<()> {
        temporary_mask.remove(SIGKILL);
        temporary_mask.remove(SIGSTOP);
        let thread = self.thread_mut(task_id)?;
        if thread.suspend_restore_mask
                 .is_some()
        {
            return Err(SignalError::InvalidHow);
        }
        thread.suspend_restore_mask = Some(thread.mask);
        thread.mask = temporary_mask;
        Ok(())
    }

    pub fn begin_signal_wait(&mut self, task_id : usize, wait_set : SignalSet) -> SignalResult<()> {
        self.thread_mut(task_id)?
            .waiting_for = Some(wait_set);
        Ok(())
    }

    pub fn end_signal_wait(&mut self, task_id : usize) -> SignalResult<()> {
        self.thread_mut(task_id)?
            .waiting_for = None;
        Ok(())
    }

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

    fn classify(&self, pid : usize, sig : usize) -> SignalResult<SignalDelivery> {
        if !valid_signal(sig) {
            return Err(SignalError::InvalidSignal);
        }
        let action = self.processes
                         .get(&pid)
                         .ok_or(SignalError::NoSuchProcess)?
                         .action(sig);
        if action.is_ignore() || action.is_default() && default_ignored(sig) {
            Ok(SignalDelivery::Ignored)
        } else if immutable_signal(sig) || action.is_default() && default_terminates(sig) {
            Ok(SignalDelivery::Terminate)
        } else {
            Ok(SignalDelivery::Pending)
        }
    }

    pub fn send_thread(&mut self, task_id : usize, sig : usize) -> SignalResult<SignalDispatch> {
        let pid = self.thread(task_id)?
                      .pid;
        match self.classify(pid, sig)? {
            SignalDelivery::Ignored => Ok(SignalDispatch::ignored()),
            SignalDelivery::Terminate => Ok(SignalDispatch::terminate(Some(task_id))),
            SignalDelivery::Pending => {
                self.thread_mut(task_id)?
                    .pending
                    .insert(sig);
                Ok(SignalDispatch::pending(Some(task_id)))
            }
        }
    }

    pub fn send_process(&mut self, pid : usize, sig : usize) -> SignalResult<SignalDispatch> {
        let delivery = self.classify(pid, sig)?;
        if delivery == SignalDelivery::Ignored {
            return Ok(SignalDispatch::ignored());
        }
        let target = self.threads
                         .iter()
                         .filter(|(_, thread)| {
                             thread.pid == pid &&
                             (delivery == SignalDelivery::Terminate ||
                              !thread.mask
                                     .contains(sig) ||
                              thread.waiting_for
                                    .is_some_and(|set| set.contains(sig)))
                         })
                         .min_by_key(|(_, thread)| thread.tid)
                         .map(|(task_id, _)| *task_id);
        if delivery == SignalDelivery::Terminate {
            return Ok(SignalDispatch::terminate(target));
        }
        self.processes
            .get_mut(&pid)
            .ok_or(SignalError::NoSuchProcess)?
            .pending
            .insert(sig);
        Ok(SignalDispatch::pending(target))
    }

    /// Compatibility entry: target a task directly.
    pub fn send(&mut self, task_id : usize, sig : usize) -> SignalResult<SignalDelivery> {
        Ok(self.send_thread(task_id, sig)?
               .delivery)
    }

    pub fn pending(&self, task_id : usize) -> SignalResult<SignalSet> {
        let thread = self.thread(task_id)?;
        let process = self.processes
                          .get(&thread.pid)
                          .ok_or(SignalError::NoSuchProcess)?;
        Ok(thread.pending
                 .union(process.pending))
    }

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

    pub fn take_pending(&mut self, task_id : usize, wait_set : SignalSet) -> Option<usize> {
        let thread = *self.threads
                          .get(&task_id)?;
        let thread_ready = thread.pending
                                 .intersection(wait_set);
        if let Some(sig) = thread_ready.first_signal() {
            self.threads
                .get_mut(&task_id)?
                .pending
                .remove(sig);
            return Some(sig);
        }
        let process = self.processes
                          .get_mut(&thread.pid)?;
        let sig = process.pending
                         .intersection(wait_set)
                         .first_signal()?;
        process.pending
               .remove(sig);
        Some(sig)
    }

    pub fn take_deliverable(&mut self, task_id : usize) -> Option<PendingSignal> {
        let thread = *self.threads
                          .get(&task_id)?;
        let process = self.processes
                          .get(&thread.pid)?;
        let deliverable = thread.pending
                                .union(process.pending)
                                .difference(thread.mask);
        let sig = deliverable.first_signal()?;
        let action = process.action(sig);
        if self.threads
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
        } else {
            self.processes
                .get_mut(&thread.pid)?
                .pending
                .remove(sig);
        }
        let previous_mask = thread.suspend_restore_mask
                                  .unwrap_or(thread.mask);
        let mut handler_mask = previous_mask.union(action.mask);
        if action.flags & SA_NODEFER == 0 {
            handler_mask.insert(sig);
        }
        let target_thread = self.threads
                                .get_mut(&task_id)?;
        target_thread.mask = handler_mask;
        target_thread.suspend_restore_mask = None;
        target_thread.waiting_for = None;
        if action.flags & SA_RESETHAND != 0 {
            self.processes
                .get_mut(&thread.pid)?
                .actions[sig] = SignalAction::default_action();
        }
        Some(PendingSignal { signal : sig,
                             action,
                             previous_mask })
    }

    pub fn restore_mask(&mut self, task_id : usize, mask : SignalSet) -> SignalResult<()> {
        self.replace_mask(task_id, mask)
    }

    pub fn set_timer(&mut self,
                     pid : usize,
                     which : usize,
                     spec : IntervalTimerSpec,
                     monotonic_ns : u128)
                     -> SignalResult<IntervalTimerSpec> {
        let process = self.processes
                          .get_mut(&pid)
                          .ok_or(SignalError::NoSuchProcess)?;
        let now = process.timer_clock(which, monotonic_ns)?;
        let old = process.timer(which)?
                         .remaining(now);
        let timer = process.timer_mut(which)?;
        timer.replace(spec, now);
        if which == ITIMER_REAL {
            if let Some(deadline) = timer.deadline_ns {
                self.real_deadlines
                    .entry(deadline)
                    .or_default()
                    .push(RealDeadlineEntry { pid,
                                              generation : timer.generation });
            }
        }
        Ok(old)
    }

    pub fn get_timer(&self,
                     pid : usize,
                     which : usize,
                     monotonic_ns : u128)
                     -> SignalResult<IntervalTimerSpec> {
        let process = self.processes
                          .get(&pid)
                          .ok_or(SignalError::NoSuchProcess)?;
        let now = process.timer_clock(which, monotonic_ns)?;
        Ok(process.timer(which)?
                  .remaining(now))
    }

    pub fn account_cpu(&mut self,
                       pid : usize,
                       user_delta_ns : u128,
                       total_delta_ns : u128)
                       -> SignalResult<Vec<(SignalDispatch, usize)>> {
        let process = self.processes
                          .get_mut(&pid)
                          .ok_or(SignalError::NoSuchProcess)?;
        process.user_cpu_ns = process.user_cpu_ns
                                     .saturating_add(user_delta_ns);
        process.total_cpu_ns = process.total_cpu_ns
                                      .saturating_add(total_delta_ns);
        let virtual_expired = process.virtual_timer
                                     .expire(process.user_cpu_ns);
        let prof_expired = process.prof
                                  .expire(process.total_cpu_ns);
        let mut dispatches = Vec::new();
        if virtual_expired {
            dispatches.push((self.send_process(pid, SIGVTALRM)?, SIGVTALRM));
        }
        if prof_expired {
            dispatches.push((self.send_process(pid, SIGPROF)?, SIGPROF));
        }
        Ok(dispatches)
    }

    pub fn expire_realtime(&mut self, monotonic_ns : u128) -> Vec<SignalDispatch> {
        let deadlines : Vec<u128> = self.real_deadlines
                                        .range(..=monotonic_ns)
                                        .map(|(deadline, _)| *deadline)
                                        .collect();
        let mut dispatches = Vec::new();
        for deadline in deadlines {
            let entries = self.real_deadlines
                              .remove(&deadline)
                              .unwrap_or_default();
            for entry in entries {
                let Some(process) = self.processes
                                        .get_mut(&entry.pid)
                else {
                    continue;
                };
                if process.real
                          .generation !=
                   entry.generation ||
                   process.real
                          .deadline_ns !=
                   Some(deadline)
                {
                    continue;
                }
                if !process.real
                           .expire(monotonic_ns)
                {
                    continue;
                }
                let next = process.real
                                  .deadline_ns;
                let generation = process.real
                                        .generation;
                if let Some(next_deadline) = next {
                    self.real_deadlines
                        .entry(next_deadline)
                        .or_default()
                        .push(RealDeadlineEntry { pid : entry.pid,
                                                  generation });
                }
                if let Ok(dispatch) = self.send_process(entry.pid, SIGALRM) {
                    dispatches.push(dispatch);
                }
            }
        }
        dispatches
    }
}

static SIGNAL_REGISTRY : Mutex<SignalRegistry> = Mutex::new(SignalRegistry::new());

pub fn with_registry<R>(f : impl FnOnce(&mut SignalRegistry) -> R) -> R {
    let mut registry = SIGNAL_REGISTRY.lock();
    f(&mut registry)
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

        let pending = registry.take_deliverable(100)
                              .unwrap();
        assert_eq!(pending.previous_mask, original);
        registry.restore_mask(100, pending.previous_mask)
                .unwrap();
        assert_eq!(registry.current_mask(100)
                           .unwrap(),
                   original);
    }
}

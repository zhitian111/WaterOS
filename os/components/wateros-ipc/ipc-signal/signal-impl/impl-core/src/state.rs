//! 信号注册表内部状态。

use alloc::collections::BTreeMap;

use api_v0::{
    AlternateSignalStack, IntervalTimerSpec, PosixTimerClock, SignalAction, SignalError,
    SignalResult, SignalSet, ITIMER_PROF, ITIMER_REAL, ITIMER_VIRTUAL, NSIG,
};

/// 单进程 interval timer 状态。
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct IntervalTimerState {
    pub(super) interval_ns : u128,
    pub(super) deadline_ns : Option<u128>,
    pub(super) generation : u64,
}

impl IntervalTimerState {
    pub(super) fn remaining(self, now_ns : u128) -> IntervalTimerSpec {
        IntervalTimerSpec { interval_ns : self.interval_ns,
                            value_ns : self.deadline_ns
                                           .map(|deadline| deadline.saturating_sub(now_ns))
                                           .unwrap_or(0) }
    }

    pub(super) fn replace(&mut self, spec : IntervalTimerSpec, now_ns : u128) {
        self.interval_ns = spec.interval_ns;
        self.deadline_ns = (spec.value_ns != 0).then(|| now_ns.saturating_add(spec.value_ns));
        self.generation = self.generation
                              .wrapping_add(1);
    }

    pub(super) fn expire(&mut self, now_ns : u128) -> bool {
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

#[derive(Clone, Copy, Debug)]
pub(super) struct PosixTimerState {
    pub(super) clock : PosixTimerClock,
    pub(super) signal : usize,
    pub(super) interval_ns : u128,
    pub(super) deadline_ns : Option<u128>,
    pub(super) overrun : i32,
}

impl PosixTimerState {
    pub(super) fn new(clock : PosixTimerClock, signal : usize) -> Self {
        Self { clock,
               signal,
               interval_ns : 0,
               deadline_ns : None,
               overrun : 0 }
    }

    pub(super) fn now(self, monotonic_ns : u128, realtime_ns : u128) -> u128 {
        match self.clock {
            PosixTimerClock::Realtime => realtime_ns,
            PosixTimerClock::Monotonic => monotonic_ns,
        }
    }

    pub(super) fn remaining(self, monotonic_ns : u128, realtime_ns : u128) -> IntervalTimerSpec {
        let now = self.now(monotonic_ns, realtime_ns);
        IntervalTimerSpec { interval_ns : self.interval_ns,
                            value_ns : self.deadline_ns
                                           .map(|deadline| deadline.saturating_sub(now))
                                           .unwrap_or(0) }
    }
}

/// 单进程信号状态：disposition、pending、timer 与 CPU 时间。
#[derive(Clone, Debug)]
pub(super) struct ProcessSignalState {
    pub(super) actions : [SignalAction; NSIG],
    pub(super) pending : SignalSet,
    pub(super) real : IntervalTimerState,
    pub(super) virtual_timer : IntervalTimerState,
    pub(super) prof : IntervalTimerState,
    pub(super) posix_timers : BTreeMap<usize, PosixTimerState>,
    pub(super) next_posix_timer_id : usize,
    pub(super) user_cpu_ns : u128,
    pub(super) total_cpu_ns : u128,
}

impl ProcessSignalState {
    pub(super) fn new() -> Self {
        Self { actions : [SignalAction::default_action(); NSIG],
               pending : SignalSet::empty(),
               real : IntervalTimerState::default(),
               virtual_timer : IntervalTimerState::default(),
               prof : IntervalTimerState::default(),
               posix_timers : BTreeMap::new(),
               next_posix_timer_id : 0,
               user_cpu_ns : 0,
               total_cpu_ns : 0 }
    }

    pub(super) fn action(&self, sig : usize) -> SignalAction { self.actions[sig] }

    pub(super) fn timer(&self, which : usize) -> SignalResult<&IntervalTimerState> {
        match which {
            ITIMER_REAL => Ok(&self.real),
            ITIMER_VIRTUAL => Ok(&self.virtual_timer),
            ITIMER_PROF => Ok(&self.prof),
            _ => Err(SignalError::InvalidTimer),
        }
    }

    pub(super) fn timer_mut(&mut self, which : usize) -> SignalResult<&mut IntervalTimerState> {
        match which {
            ITIMER_REAL => Ok(&mut self.real),
            ITIMER_VIRTUAL => Ok(&mut self.virtual_timer),
            ITIMER_PROF => Ok(&mut self.prof),
            _ => Err(SignalError::InvalidTimer),
        }
    }

    pub(super) fn timer_clock(&self, which : usize, monotonic_ns : u128) -> SignalResult<u128> {
        match which {
            ITIMER_REAL => Ok(monotonic_ns),
            ITIMER_VIRTUAL => Ok(self.user_cpu_ns),
            ITIMER_PROF => Ok(self.total_cpu_ns),
            _ => Err(SignalError::InvalidTimer),
        }
    }
}

/// 单线程信号状态：mask、pending、临时 mask 与备用信号栈。
#[derive(Clone, Copy, Debug)]
pub(super) struct ThreadSignalState {
    pub(super) pid : usize,
    pub(super) tid : usize,
    pub(super) mask : SignalSet,
    pub(super) pending : SignalSet,
    pub(super) suspend_restore_mask : Option<SignalSet>,
    pub(super) poll_restore_mask : Option<SignalSet>,
    pub(super) waiting_for : Option<SignalSet>,
    pub(super) alternate_stack : AlternateSignalStack,
}

impl ThreadSignalState {
    pub(super) fn new(pid : usize, tid : usize, inherited_mask : SignalSet) -> Self {
        Self { pid,
               tid,
               mask : inherited_mask,
               pending : SignalSet::empty(),
               suspend_restore_mask : None,
               poll_restore_mask : None,
               waiting_for : None,
               alternate_stack : AlternateSignalStack::default() }
    }
}

/// `generation` 用于丢弃已被替换的 `ITIMER_REAL` deadline。
#[derive(Clone, Copy, Debug)]
pub(super) struct RealDeadlineEntry {
    pub(super) pid : usize,
    pub(super) generation : u64,
}

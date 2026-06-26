#![no_std]
//! Signal API v0: Linux-like signal numbers, masks and `rt_sigaction` state.

pub const NSIG: usize = 64;
pub const SIG_DFL: usize = 0;
pub const SIG_IGN: usize = 1;
pub const SIG_BLOCK: usize = 0;
pub const SIG_UNBLOCK: usize = 1;
pub const SIG_SETMASK: usize = 2;

pub const SIGHUP: usize = 1;
pub const SIGINT: usize = 2;
pub const SIGILL: usize = 4;
pub const SIGBUS: usize = 7;
pub const SIGFPE: usize = 8;
pub const SIGKILL: usize = 9;
pub const SIGUSR1: usize = 10;
pub const SIGSEGV: usize = 11;
pub const SIGUSR2: usize = 12;
pub const SIGALRM: usize = 14;
pub const SIGTERM: usize = 15;
pub const SIGPIPE: usize = 13;
pub const SIGCHLD: usize = 17;
pub const SIGSTOP: usize = 19;
pub const SIGURG: usize = 23;
pub const SIGVTALRM: usize = 26;
pub const SIGPROF: usize = 27;
pub const SIGWINCH: usize = 28;

pub const SA_NOCLDSTOP: usize = 0x0000_0001;
pub const SA_NOCLDWAIT: usize = 0x0000_0002;
pub const SA_SIGINFO: usize = 0x0000_0004;
pub const SA_ONSTACK: usize = 0x0800_0000;
pub const SA_RESTART: usize = 0x1000_0000;
pub const SA_NODEFER: usize = 0x4000_0000;
pub const SA_RESETHAND: usize = 0x8000_0000;
pub const SA_RESTORER: usize = 0x0400_0000;

pub const ITIMER_REAL: usize = 0;
pub const ITIMER_VIRTUAL: usize = 1;
pub const ITIMER_PROF: usize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalError {
    InvalidSignal,
    InvalidHow,
    NoSuchTask,
    NoSuchProcess,
    InvalidTimer,
}

pub type SignalResult<T> = Result<T, SignalError>;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SignalSet(u64);

impl SignalSet {
    pub const fn empty() -> Self { Self(0) }

    pub const fn from_bits(bits: u64) -> Self { Self(bits) }

    pub const fn bits(self) -> u64 { self.0 }

    pub const fn contains(self, sig: usize) -> bool {
        match signal_bit(sig) {
            Some(bit) => self.0 & bit != 0,
            None => false,
        }
    }

    pub fn insert(&mut self, sig: usize) {
        if let Some(bit) = signal_bit(sig) {
            self.0 |= bit;
        }
    }

    pub fn remove(&mut self, sig: usize) {
        if let Some(bit) = signal_bit(sig) {
            self.0 &= !bit;
        }
    }

    pub fn union(self, other: Self) -> Self { Self(self.0 | other.0) }

    pub fn difference(self, other: Self) -> Self { Self(self.0 & !other.0) }

    pub fn intersection(self, other: Self) -> Self { Self(self.0 & other.0) }

    pub const fn is_empty(self) -> bool { self.0 == 0 }

    pub fn first_signal(self) -> Option<usize> {
        if self.0 == 0 {
            return None;
        }
        Some(self.0.trailing_zeros() as usize + 1)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct SignalAction {
    pub handler: usize,
    pub flags: usize,
    pub restorer: usize,
    pub mask: SignalSet,
}

impl SignalAction {
    pub const fn default_action() -> Self {
        Self { handler: SIG_DFL,
               flags: 0,
               restorer: 0,
               mask: SignalSet::empty() }
    }

    pub const fn ignore() -> Self {
        Self { handler: SIG_IGN,
               flags: 0,
               restorer: 0,
               mask: SignalSet::empty() }
    }

    pub const fn is_default(self) -> bool { self.handler == SIG_DFL }

    pub const fn is_ignore(self) -> bool { self.handler == SIG_IGN }

    pub const fn has_user_handler(self) -> bool { self.handler > SIG_IGN }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalDelivery {
    Ignored,
    Pending,
    Terminate,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct IntervalTimerSpec {
    pub interval_ns: u128,
    pub value_ns: u128,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PendingSignal {
    pub signal: usize,
    pub action: SignalAction,
    pub previous_mask: SignalSet,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SignalDispatch {
    pub delivery: SignalDelivery,
    pub target_task_id: Option<usize>,
}

impl SignalDispatch {
    pub const fn ignored() -> Self {
        Self { delivery: SignalDelivery::Ignored,
               target_task_id: None }
    }

    pub const fn terminate(target_task_id: Option<usize>) -> Self {
        Self { delivery: SignalDelivery::Terminate,
               target_task_id }
    }

    pub const fn pending(target_task_id: Option<usize>) -> Self {
        Self { delivery: SignalDelivery::Pending,
               target_task_id }
    }
}

pub const fn valid_signal(sig: usize) -> bool { sig > 0 && sig < NSIG }

pub const fn valid_itimer(which: usize) -> bool {
    matches!(which, ITIMER_REAL | ITIMER_VIRTUAL | ITIMER_PROF)
}

pub const fn immutable_signal(sig: usize) -> bool { sig == SIGKILL || sig == SIGSTOP }

pub const fn signal_bit(sig: usize) -> Option<u64> {
    if valid_signal(sig) {
        Some(1u64 << (sig - 1))
    } else {
        None
    }
}

pub const fn default_ignored(sig: usize) -> bool {
    // SIGCHLD 默认虽为“忽略”语义，但子进程退出时仍须入队 pending，
    // 否则 busybox `cmd &; wait`（WNOHANG + sigsuspend）无法被唤醒。
    sig == SIGURG || sig == SIGWINCH
}

pub const fn default_terminates(sig: usize) -> bool {
    // SIGCHLD 默认既不忽略也不终止父进程，而是入队 pending 供 wait/sigsuspend 观察。
    valid_signal(sig) && !default_ignored(sig) && sig != SIGSTOP && sig != SIGCHLD
}

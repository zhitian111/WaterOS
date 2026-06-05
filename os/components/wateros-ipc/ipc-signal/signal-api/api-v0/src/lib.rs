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
pub const SIGKILL: usize = 9;
pub const SIGUSR1: usize = 10;
pub const SIGUSR2: usize = 12;
pub const SIGALRM: usize = 14;
pub const SIGTERM: usize = 15;
pub const SIGCHLD: usize = 17;
pub const SIGSTOP: usize = 19;
pub const SIGURG: usize = 23;
pub const SIGWINCH: usize = 28;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalError {
    InvalidSignal,
    InvalidHow,
    NoSuchTask,
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

pub const fn valid_signal(sig: usize) -> bool { sig > 0 && sig < NSIG }

pub const fn immutable_signal(sig: usize) -> bool { sig == SIGKILL || sig == SIGSTOP }

pub const fn signal_bit(sig: usize) -> Option<u64> {
    if valid_signal(sig) {
        Some(1u64 << (sig - 1))
    } else {
        None
    }
}

pub const fn default_ignored(sig: usize) -> bool {
    sig == SIGCHLD || sig == SIGURG || sig == SIGWINCH
}

pub const fn default_terminates(sig: usize) -> bool {
    valid_signal(sig) && !default_ignored(sig) && sig != SIGSTOP
}

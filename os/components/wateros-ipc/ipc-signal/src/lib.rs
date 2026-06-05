#![no_std]
//! Minimal signal IPC state: signal actions, per-task masks and pending sets.

extern crate alloc;

use alloc::collections::BTreeMap;
use spin::Mutex;

pub mod api {
    pub use ::api_v0::*;
}

pub use api_v0::{
    default_ignored, default_terminates, immutable_signal, valid_signal, SignalAction,
    SignalDelivery, SignalError, SignalResult, SignalSet, NSIG, SIG_BLOCK, SIG_DFL, SIG_IGN,
    SIG_SETMASK, SIG_UNBLOCK,
};

#[derive(Clone, Debug)]
struct TaskSignalState {
    mask: SignalSet,
    pending: SignalSet,
    actions: [SignalAction; NSIG],
}

impl TaskSignalState {
    fn new() -> Self {
        Self { mask: SignalSet::empty(),
               pending: SignalSet::empty(),
               actions: [SignalAction::default_action(); NSIG] }
    }

    fn action(&self, sig: usize) -> SignalAction {
        if valid_signal(sig) {
            self.actions[sig]
        } else {
            SignalAction::default_action()
        }
    }

    fn set_action(&mut self, sig: usize, action: SignalAction) -> SignalResult<SignalAction> {
        if !valid_signal(sig) || immutable_signal(sig) {
            return Err(SignalError::InvalidSignal);
        }
        let old = self.actions[sig];
        self.actions[sig] = action;
        Ok(old)
    }

    fn update_mask(&mut self, how: usize, set: Option<SignalSet>) -> SignalResult<SignalSet> {
        let old = self.mask;
        let Some(mut set) = set else {
            return Ok(old);
        };
        set.remove(api_v0::SIGKILL);
        set.remove(api_v0::SIGSTOP);
        self.mask = match how {
            SIG_BLOCK => self.mask.union(set),
            SIG_UNBLOCK => self.mask.difference(set),
            SIG_SETMASK => set,
            _ => return Err(SignalError::InvalidHow),
        };
        Ok(old)
    }

    fn send(&mut self, sig: usize) -> SignalResult<SignalDelivery> {
        if !valid_signal(sig) {
            return Err(SignalError::InvalidSignal);
        }
        let action = self.action(sig);
        if action.is_ignore() || action.is_default() && default_ignored(sig) {
            return Ok(SignalDelivery::Ignored);
        }
        if immutable_signal(sig) || action.is_default() && default_terminates(sig) && !self.mask.contains(sig) {
            return Ok(SignalDelivery::Terminate);
        }
        self.pending.insert(sig);
        Ok(SignalDelivery::Pending)
    }

    fn take_pending(&mut self, wait_set: SignalSet) -> Option<usize> {
        let ready = self.pending.intersection(wait_set);
        let sig = ready.first_signal()?;
        self.pending.remove(sig);
        Some(sig)
    }
}

#[derive(Default)]
pub struct SignalRegistry {
    tasks: BTreeMap<usize, TaskSignalState>,
}

impl SignalRegistry {
    pub const fn new() -> Self { Self { tasks: BTreeMap::new() } }

    fn task_mut(&mut self, task_id: usize) -> &mut TaskSignalState {
        self.tasks
            .entry(task_id)
            .or_insert_with(TaskSignalState::new)
    }

    pub fn get_action(&mut self, task_id: usize, sig: usize) -> SignalResult<SignalAction> {
        if !valid_signal(sig) {
            return Err(SignalError::InvalidSignal);
        }
        Ok(self.task_mut(task_id).action(sig))
    }

    pub fn set_action(
        &mut self,
        task_id: usize,
        sig: usize,
        action: SignalAction,
    ) -> SignalResult<SignalAction> {
        self.task_mut(task_id).set_action(sig, action)
    }

    pub fn update_mask(
        &mut self,
        task_id: usize,
        how: usize,
        set: Option<SignalSet>,
    ) -> SignalResult<SignalSet> {
        self.task_mut(task_id).update_mask(how, set)
    }

    pub fn send(&mut self, task_id: usize, sig: usize) -> SignalResult<SignalDelivery> {
        self.task_mut(task_id).send(sig)
    }

    pub fn take_pending(&mut self, task_id: usize, wait_set: SignalSet) -> Option<usize> {
        self.task_mut(task_id).take_pending(wait_set)
    }

    pub fn drop_task(&mut self, task_id: usize) {
        self.tasks.remove(&task_id);
    }
}

static SIGNAL_REGISTRY: Mutex<SignalRegistry> = Mutex::new(SignalRegistry::new());

pub fn with_registry<R>(f: impl FnOnce(&mut SignalRegistry) -> R) -> R {
    let mut registry = SIGNAL_REGISTRY.lock();
    f(&mut registry)
}

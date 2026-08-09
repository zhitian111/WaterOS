//! Lock-free single-publication slot for a diagnostic IRQ runtime.
//!
//! Reservation happens before hardware activation.  Once reserved, commit is
//! infallible, so no publication race can strand enabled interrupt sources.

use core::{cell::UnsafeCell,
           mem::{ManuallyDrop, MaybeUninit},
           sync::atomic::{AtomicU8, Ordering}};

const EMPTY : u8 = 0;
const RESERVED : u8 = 1;
const LIVE : u8 = 2;
const SERVICING : u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotError {
    Reserved,
    AlreadyLive,
    Empty,
    Busy,
}

pub struct DiagnosticRuntimeSlot<T> {
    state : AtomicU8,
    value : UnsafeCell<MaybeUninit<T>>,
}

// State CAS operations guarantee exclusive mutation of the stored value.
unsafe impl<T : Send> Sync for DiagnosticRuntimeSlot<T> {}

impl<T> DiagnosticRuntimeSlot<T> {
    pub const fn new() -> Self {
        Self { state : AtomicU8::new(EMPTY),
               value : UnsafeCell::new(MaybeUninit::uninit()) }
    }

    pub fn reserve(&self) -> Result<RuntimeReservation<'_, T>, SlotError> {
        match self.state.compare_exchange(EMPTY, RESERVED, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => Ok(RuntimeReservation { slot : self }),
            Err(RESERVED) => Err(SlotError::Reserved),
            Err(LIVE) => Err(SlotError::AlreadyLive),
            Err(SERVICING) => Err(SlotError::Busy),
            Err(_) => Err(SlotError::Busy),
        }
    }

    pub fn with_live_mut<R>(&self, f : impl FnOnce(&mut T) -> R) -> Result<R, SlotError> {
        match self.state.compare_exchange(LIVE, SERVICING, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => {}
            Err(EMPTY) => return Err(SlotError::Empty),
            Err(RESERVED) | Err(SERVICING) => return Err(SlotError::Busy),
            Err(_) => return Err(SlotError::Busy),
        }
        let guard = ServiceGuard { slot : self };
        // SAFETY: LIVE->SERVICING CAS grants this guard exclusive access and
        // commit publishes the initialized value before its Release store.
        let value = unsafe { (&mut *self.value.get()).assume_init_mut() };
        let result = f(value);
        drop(guard);
        Ok(result)
    }
}

impl<T> Default for DiagnosticRuntimeSlot<T> {
    fn default() -> Self { Self::new() }
}

impl<T> Drop for DiagnosticRuntimeSlot<T> {
    fn drop(&mut self) {
        if *self.state.get_mut() == LIVE {
            // SAFETY: exclusive `&mut self` and LIVE prove initialization.
            unsafe { self.value.get_mut().assume_init_drop(); }
        }
    }
}

pub struct RuntimeReservation<'a, T> {
    slot : &'a DiagnosticRuntimeSlot<T>,
}

impl<T> RuntimeReservation<'_, T> {
    pub fn commit(self, value : T) {
        let this = ManuallyDrop::new(self);
        // SAFETY: the reservation exclusively owns the uninitialized slot.
        unsafe { (&mut *this.slot.value.get()).write(value); }
        this.slot.state.store(LIVE, Ordering::Release);
    }
}

impl<T> Drop for RuntimeReservation<'_, T> {
    fn drop(&mut self) {
        self.slot.state.store(EMPTY, Ordering::Release);
    }
}

struct ServiceGuard<'a, T> {
    slot : &'a DiagnosticRuntimeSlot<T>,
}

impl<T> Drop for ServiceGuard<'_, T> {
    fn drop(&mut self) {
        self.slot.state.store(LIVE, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reservation_drop_reopens_slot_and_commit_is_single_publication() {
        let slot = DiagnosticRuntimeSlot::new();
        assert_eq!(slot.with_live_mut(|_: &mut u32| ()), Err(SlotError::Empty));
        let reservation = slot.reserve().unwrap();
        assert_eq!(slot.reserve().err(), Some(SlotError::Reserved));
        drop(reservation);
        slot.reserve().unwrap().commit(41u32);
        assert_eq!(slot.reserve().err(), Some(SlotError::AlreadyLive));
        assert_eq!(slot.with_live_mut(|value| { *value += 1; *value }), Ok(42));
    }

    #[test]
    fn servicing_rejects_reentrant_access_without_locking() {
        let slot = DiagnosticRuntimeSlot::new();
        slot.reserve().unwrap().commit(7u8);
        let result = slot.with_live_mut(|value| {
            assert_eq!(slot.with_live_mut(|_| ()), Err(SlotError::Busy));
            *value
        });
        assert_eq!(result, Ok(7));
        assert_eq!(slot.with_live_mut(|value| *value), Ok(7));
    }
}

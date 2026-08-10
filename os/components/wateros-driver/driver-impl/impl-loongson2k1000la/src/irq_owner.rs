//! Fixed-capacity IRQ-to-device ownership slots.
//!
//! A handler begins by moving its owner out of `Ready` and leaving an
//! `InHandler` sentinel. The linear [`ActiveOwner`] must be finished to restore
//! the slot. Dropping it is intentionally fail-closed: the source remains busy
//! and cannot be consumed again.

use crate::irq_domain::{AcknowledgedIrq, GlobalIrq, IrqDisposition, MAX_GLOBAL_IRQS};
use core::sync::atomic::{AtomicU64, Ordering};

static NEXT_OWNER_GENERATION : AtomicU64 = AtomicU64::new(1);

enum OwnerSlot<O> {
    Empty,
    Ready { owner : O, generation : u64 },
    InHandler { generation : u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerError {
    AlreadyRegistered,
    NotRegistered,
    InHandler,
    InvalidCompletion,
}

#[derive(Debug, PartialEq, Eq)]
pub struct BeginFailure {
    pub error : OwnerError,
    pub acknowledged : AcknowledgedIrq,
}

#[derive(Debug)]
pub struct ActiveOwner<O> {
    irq : GlobalIrq,
    owner : O,
    generation : u64,
    acknowledged : Option<AcknowledgedIrq>,
}

pub trait IrqOwner {
    fn handle(&mut self, acknowledged : AcknowledgedIrq) -> IrqDisposition;
}

impl<O> ActiveOwner<O> {
    pub const fn irq(&self) -> GlobalIrq { self.irq }
    pub fn owner(&self) -> &O { &self.owner }
    pub fn owner_mut(&mut self) -> &mut O { &mut self.owner }

    pub fn handle(mut self) -> (Self, IrqDisposition)
    where O : IrqOwner
    {
        let acknowledged = self.acknowledged.take()
                                .expect("active IRQ evidence already consumed");
        let disposition = self.owner.handle(acknowledged);
        (self, disposition)
    }
}

pub struct FinishFailure<O> {
    pub error : OwnerError,
    pub active : ActiveOwner<O>,
}

pub struct IrqOwnerTable<O> {
    slots : [OwnerSlot<O>; MAX_GLOBAL_IRQS],
}

impl<O> IrqOwnerTable<O> {
    pub fn new() -> Self {
        Self { slots : core::array::from_fn(|_| OwnerSlot::Empty) }
    }

    pub fn register(&mut self, irq : GlobalIrq, owner : O) -> Result<(), (OwnerError, O)> {
        let slot = &mut self.slots[irq.raw() as usize];
        match slot {
            OwnerSlot::Empty => {
                let generation = NEXT_OWNER_GENERATION.fetch_add(1, Ordering::Relaxed);
                *slot = OwnerSlot::Ready { owner, generation };
                Ok(())
            }
            OwnerSlot::Ready { .. } => Err((OwnerError::AlreadyRegistered, owner)),
            OwnerSlot::InHandler { .. } => Err((OwnerError::InHandler, owner)),
        }
    }

    pub fn unregister(&mut self, irq : GlobalIrq) -> Result<O, OwnerError> {
        let slot = &mut self.slots[irq.raw() as usize];
        match core::mem::replace(slot, OwnerSlot::Empty) {
            OwnerSlot::Ready { owner, .. } => Ok(owner),
            OwnerSlot::Empty => {
                *slot = OwnerSlot::Empty;
                Err(OwnerError::NotRegistered)
            }
            OwnerSlot::InHandler { generation } => {
                *slot = OwnerSlot::InHandler { generation };
                Err(OwnerError::InHandler)
            }
        }
    }

    pub fn get(&self, irq : GlobalIrq) -> Result<&O, OwnerError> {
        match &self.slots[irq.raw() as usize] {
            OwnerSlot::Ready { owner, .. } => Ok(owner),
            OwnerSlot::Empty => Err(OwnerError::NotRegistered),
            OwnerSlot::InHandler { .. } => Err(OwnerError::InHandler),
        }
    }

    /// Mutably inspect a ready owner between interrupt service transactions.
    /// An owner in its handler remains inaccessible.
    pub fn get_mut(&mut self, irq : GlobalIrq) -> Result<&mut O, OwnerError> {
        match &mut self.slots[irq.raw() as usize] {
            OwnerSlot::Ready { owner, .. } => Ok(owner),
            OwnerSlot::Empty => Err(OwnerError::NotRegistered),
            OwnerSlot::InHandler { .. } => Err(OwnerError::InHandler),
        }
    }

    pub fn begin(&mut self, acknowledged : AcknowledgedIrq)
                 -> Result<ActiveOwner<O>, BeginFailure> {
        let irq = acknowledged.irq();
        let slot = &mut self.slots[irq.raw() as usize];
        match core::mem::replace(slot, OwnerSlot::Empty) {
            OwnerSlot::Ready { owner, generation } => {
                *slot = OwnerSlot::InHandler { generation };
                Ok(ActiveOwner { irq,
                                 owner,
                                 generation,
                                 acknowledged : Some(acknowledged) })
            }
            OwnerSlot::Empty => {
                *slot = OwnerSlot::Empty;
                Err(BeginFailure { error : OwnerError::NotRegistered, acknowledged })
            }
            OwnerSlot::InHandler { generation } => {
                *slot = OwnerSlot::InHandler { generation };
                Err(BeginFailure { error : OwnerError::InHandler, acknowledged })
            }
        }
    }

    pub fn finish(&mut self, active : ActiveOwner<O>)
                  -> Result<(), FinishFailure<O>> {
        let index = active.irq.raw() as usize;
        if !matches!(self.slots[index],
                     OwnerSlot::InHandler { generation } if generation == active.generation) {
            return Err(FinishFailure { error : OwnerError::InvalidCompletion, active });
        }
        self.slots[index] = OwnerSlot::Ready { owner : active.owner,
                                               generation : active.generation };
        Ok(())
    }

    pub fn is_busy(&self, irq : GlobalIrq) -> bool {
        matches!(self.slots[irq.raw() as usize], OwnerSlot::InHandler { .. })
    }
}

impl<O> Default for IrqOwnerTable<O> {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn irq(local : u32) -> GlobalIrq { GlobalIrq::from_bank_local(0, local).unwrap() }
    fn acknowledged(irq : GlobalIrq) -> AcknowledgedIrq {
        AcknowledgedIrq::after_mask_ack(irq)
    }

    #[test]
    fn register_begin_mutate_finish_and_unregister_preserve_one_owner() {
        let source = irq(7);
        let mut table = IrqOwnerTable::new();
        assert_eq!(table.register(source, 10u32), Ok(()));
        let duplicate = table.register(source, 20).unwrap_err();
        assert_eq!(duplicate, (OwnerError::AlreadyRegistered, 20));
        let mut active = table.begin(acknowledged(source)).unwrap();
        assert_eq!(active.irq(), source);
        *active.owner_mut() += 5;
        assert!(table.is_busy(source));
        let reentry = table.begin(acknowledged(source)).unwrap_err();
        assert_eq!(reentry.error, OwnerError::InHandler);
        assert_eq!(reentry.acknowledged.irq(), source);
        assert_eq!(table.unregister(source), Err(OwnerError::InHandler));
        table.finish(active).unwrap_or_else(|_| panic!("finish failed"));
        assert!(!table.is_busy(source));
        *table.get_mut(source).unwrap() += 1;
        assert_eq!(table.unregister(source), Ok(16));
    }

    #[test]
    fn unregistered_and_dropped_active_owner_fail_closed() {
        let source = irq(9);
        let mut table : IrqOwnerTable<u32> = IrqOwnerTable::new();
        let failure = table.begin(acknowledged(source)).unwrap_err();
        assert_eq!(failure.error, OwnerError::NotRegistered);
        assert_eq!(failure.acknowledged.irq(), source);

        table.register(source, 42).unwrap();
        let active = table.begin(acknowledged(source)).unwrap();
        drop(active);
        assert!(table.is_busy(source));
        let failure = table.begin(acknowledged(source)).unwrap_err();
        assert_eq!(failure.error, OwnerError::InHandler);
        assert_eq!(table.unregister(source), Err(OwnerError::InHandler));
    }

    #[test]
    fn active_owner_cannot_finish_into_another_table() {
        let source = irq(11);
        let mut first = IrqOwnerTable::new();
        let mut second = IrqOwnerTable::new();
        first.register(source, 1u32).unwrap();
        second.register(source, 2u32).unwrap();
        let first_active = first.begin(acknowledged(source)).unwrap();
        let second_active = second.begin(acknowledged(source)).unwrap();
        let failure = second.finish(first_active)
                            .err()
                            .expect("cross-table finish succeeded");
        assert_eq!(failure.error, OwnerError::InvalidCompletion);
        assert!(first.is_busy(source));
        assert!(second.is_busy(source));
        first.finish(failure.active).unwrap_or_else(|_| panic!("first finish failed"));
        second.finish(second_active).unwrap_or_else(|_| panic!("second finish failed"));
        assert_eq!(first.unregister(source), Ok(1));
        assert_eq!(second.unregister(source), Ok(2));
    }
}

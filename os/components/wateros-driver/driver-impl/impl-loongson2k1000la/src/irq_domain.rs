//! Pure two-bank LIOINTC IRQ domain and dispatch table.
//!
//! The table has no allocation or locking in dispatch. Registration requires
//! exclusive access and must finish before hardware interrupts are enabled.

pub const IRQS_PER_BANK : u32 = 32;
pub const MAX_BANKS : usize = 2;
pub const MAX_GLOBAL_IRQS : usize = IRQS_PER_BANK as usize * MAX_BANKS;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GlobalIrq(u8);

impl GlobalIrq {
    pub fn from_bank_local(bank : usize, local : u32) -> Result<Self, DomainError> {
        if bank >= MAX_BANKS || local >= IRQS_PER_BANK {
            return Err(DomainError::OutOfRange);
        }
        Ok(Self((bank * IRQS_PER_BANK as usize + local as usize) as u8))
    }

    pub const fn raw(self) -> u8 { self.0 }
    pub const fn bank(self) -> usize { self.0 as usize / IRQS_PER_BANK as usize }
    pub const fn local(self) -> u32 { self.0 as u32 % IRQS_PER_BANK }
}

/// Evidence that a LIOINTC source was masked/acknowledged before dispatch.
#[derive(Debug, PartialEq, Eq)]
pub struct AcknowledgedIrq {
    irq : GlobalIrq,
}

impl AcknowledgedIrq {
    pub const fn irq(&self) -> GlobalIrq { self.irq }

    pub(crate) const fn after_mask_ack(irq : GlobalIrq) -> Self { Self { irq } }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainError {
    InvalidBankCount,
    OutOfRange,
    AlreadyRegistered,
    NotRegistered,
}

/// Evidence that the device-level interrupt condition for one source was
/// cleared after its LIOINTC line had been masked/acknowledged.
#[derive(Debug, PartialEq, Eq)]
pub struct DeviceAckedIrq {
    irq : GlobalIrq,
}

impl DeviceAckedIrq {
    pub const fn irq(&self) -> GlobalIrq { self.irq }

    #[allow(dead_code)]
    pub(crate) const fn after_device_clear(irq : GlobalIrq) -> Self { Self { irq } }
}

#[derive(Debug, PartialEq, Eq)]
pub enum IrqDisposition {
    KeepMasked,
    Rearm(DeviceAckedIrq),
}

pub type IrqHandler = fn(AcknowledgedIrq) -> IrqDisposition;

#[derive(Debug, PartialEq, Eq)]
pub struct UnhandledIrq {
    pub error : DomainError,
    pub acknowledged : AcknowledgedIrq,
}

pub struct LioIntcDomain {
    bank_count : u8,
    handlers : [Option<IrqHandler>; MAX_GLOBAL_IRQS],
}

impl LioIntcDomain {
    pub fn new(bank_count : usize) -> Result<Self, DomainError> {
        if bank_count == 0 || bank_count > MAX_BANKS {
            return Err(DomainError::InvalidBankCount);
        }
        Ok(Self { bank_count : bank_count as u8,
                  handlers : [None; MAX_GLOBAL_IRQS] })
    }

    fn validate(&self, irq : GlobalIrq) -> Result<usize, DomainError> {
        (irq.bank() < self.bank_count as usize).then_some(irq.raw() as usize)
                                               .ok_or(DomainError::OutOfRange)
    }

    pub fn register(&mut self, irq : GlobalIrq, handler : IrqHandler) -> Result<(), DomainError> {
        let index = self.validate(irq)?;
        if self.handlers[index].is_some() {
            return Err(DomainError::AlreadyRegistered);
        }
        self.handlers[index] = Some(handler);
        Ok(())
    }

    pub fn unregister(&mut self, irq : GlobalIrq) -> Result<IrqHandler, DomainError> {
        let index = self.validate(irq)?;
        self.handlers[index].take()
                            .ok_or(DomainError::NotRegistered)
    }

    /// Dispatch one source only after its controller has masked/acknowledged it.
    /// An unregistered source returns the linear evidence so the caller can
    /// keep the line masked and report or recover it deliberately.
    pub fn dispatch(&self, acknowledged : AcknowledgedIrq)
                    -> Result<IrqDisposition, UnhandledIrq> {
        let irq = acknowledged.irq();
        let index = match self.validate(irq) {
            Ok(index) => index,
            Err(error) => return Err(UnhandledIrq { error, acknowledged }),
        };
        match self.handlers[index] {
            Some(handler) => {
                Ok(handler(acknowledged))
            }
            None => Err(UnhandledIrq { error : DomainError::NotRegistered,
                                      acknowledged }),
        }
    }
}

#[cfg(test)]
mod tests {
    use core::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static VISITED : AtomicU64 = AtomicU64::new(0);

    fn record(acknowledged : AcknowledgedIrq) -> IrqDisposition {
        VISITED.fetch_or(1u64 << acknowledged.irq().raw(), Ordering::Relaxed);
        IrqDisposition::KeepMasked
    }

    #[test]
    fn maps_bank_local_boundaries() {
        let first = GlobalIrq::from_bank_local(0, 0).unwrap();
        let last = GlobalIrq::from_bank_local(1, 31).unwrap();
        assert_eq!((first.raw(), first.bank(), first.local()),
                   (0, 0, 0));
        assert_eq!((last.raw(), last.bank(), last.local()),
                   (63, 1, 31));
        assert_eq!(GlobalIrq::from_bank_local(2, 0),
                   Err(DomainError::OutOfRange));
        assert_eq!(GlobalIrq::from_bank_local(0, 32),
                   Err(DomainError::OutOfRange));
    }

    #[test]
    fn dispatches_only_mask_ack_evidence_and_returns_unhandled_token() {
        VISITED.store(0, Ordering::Relaxed);
        let mut domain = LioIntcDomain::new(2).unwrap();
        domain.register(GlobalIrq::from_bank_local(1, 0).unwrap(),
                        record)
              .unwrap();
        let handled = AcknowledgedIrq::after_mask_ack(
            GlobalIrq::from_bank_local(1, 0).unwrap());
        assert_eq!(domain.dispatch(handled), Ok(IrqDisposition::KeepMasked));
        let missing_irq = GlobalIrq::from_bank_local(1, 7).unwrap();
        let failure = domain.dispatch(AcknowledgedIrq::after_mask_ack(missing_irq))
                            .unwrap_err();
        assert_eq!(failure.error, DomainError::NotRegistered);
        assert_eq!(failure.acknowledged.irq(), missing_irq);
        assert_eq!(VISITED.load(Ordering::Relaxed), 1u64 << 32);
    }

    #[test]
    fn rejects_duplicate_and_supports_unregister() {
        let irq = GlobalIrq::from_bank_local(0, 5).unwrap();
        let mut domain = LioIntcDomain::new(1).unwrap();
        domain.register(irq, record)
              .unwrap();
        assert_eq!(domain.register(irq, record),
                   Err(DomainError::AlreadyRegistered));
        assert!(domain.unregister(irq)
                      .is_ok());
        assert_eq!(domain.unregister(irq),
                   Err(DomainError::NotRegistered));
    }

    #[test]
    fn validates_active_bank_count() {
        assert!(matches!(LioIntcDomain::new(0),
                         Err(DomainError::InvalidBankCount)));
        assert!(matches!(LioIntcDomain::new(3),
                         Err(DomainError::InvalidBankCount)));
        let domain = LioIntcDomain::new(1).unwrap();
        let irq = GlobalIrq::from_bank_local(1, 0).unwrap();
        let failure = domain.dispatch(AcknowledgedIrq::after_mask_ack(irq))
                            .unwrap_err();
        assert_eq!(failure.error, DomainError::OutOfRange);
        assert_eq!(failure.acknowledged.irq(), irq);
    }
}

//! Allocation-free board IRQ service core.
//!
//! Runtime assembly with volatile controllers is deliberately separate and
//! remains `UNVERIFIED_ON_HARDWARE`. This module makes snapshot expansion,
//! mask/ack ordering and token dispatch host-testable.

use api_v0::DriverError;

use crate::{irq_domain::{DomainError, GlobalIrq, IrqHandler, LioIntcDomain, MAX_BANKS},
            liointc::{LioIntc, RegisterIo}};

const HWI_LINES : usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeError {
    InvalidSnapshot,
    InvalidCore,
    UnmappedParent,
    MissingController,
    Controller(DriverError),
    Domain(DomainError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ServiceReport {
    pub parent_lines : u8,
    pub masked_sources : u8,
    pub handled_sources : u8,
    pub unhandled_sources : u8,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ServiceFailure {
    pub error : RuntimeError,
    pub report : ServiceReport,
}

pub struct BoardIrqRuntime<I> {
    controllers : [Option<LioIntc<I>>; MAX_BANKS],
    parent_banks : [Option<u8>; HWI_LINES],
    domain : LioIntcDomain,
}

impl<I : RegisterIo> BoardIrqRuntime<I> {
    pub fn new(controllers : [Option<LioIntc<I>>; MAX_BANKS],
               parent_banks : [Option<u8>; HWI_LINES])
               -> Result<Self, RuntimeError> {
        for (bank, controller) in controllers.iter().enumerate() {
            if controller.as_ref().is_some_and(|controller| controller.bank() != bank) {
                return Err(RuntimeError::MissingController);
            }
        }
        for bank in parent_banks.iter().flatten() {
            if *bank as usize >= MAX_BANKS || controllers[*bank as usize].is_none() {
                return Err(RuntimeError::MissingController);
            }
        }
        Ok(Self { controllers,
                  parent_banks,
                  domain : LioIntcDomain::new(MAX_BANKS).map_err(RuntimeError::Domain)? })
    }

    pub fn register(&mut self, irq : GlobalIrq, handler : IrqHandler)
                    -> Result<(), RuntimeError> {
        self.domain.register(irq, handler).map_err(RuntimeError::Domain)
    }

    pub fn into_controllers(self) -> [Option<LioIntc<I>>; MAX_BANKS] {
        self.controllers
    }

    pub fn service(&mut self, snapshot : usize, core : usize)
                   -> Result<ServiceReport, ServiceFailure> {
        if snapshot == 0 || snapshot & !0xff != 0 {
            return Err(ServiceFailure { error : RuntimeError::InvalidSnapshot,
                                        report : ServiceReport::default() });
        }
        let mut report = ServiceReport::default();
        let mut remaining = snapshot as u8;
        let mut serviced_banks = 0u8;
        while remaining != 0 {
            let line = remaining.trailing_zeros() as usize;
            remaining &= !(1 << line);
            let bank = match self.parent_banks[line] {
                Some(bank) => bank as usize,
                None => return Err(ServiceFailure { error : RuntimeError::UnmappedParent,
                                                    report }),
            };
            report.parent_lines = report.parent_lines.saturating_add(1);
            if serviced_banks & (1 << bank) != 0 { continue; }
            serviced_banks |= 1 << bank;
            let controller = match self.controllers[bank].as_mut() {
                Some(controller) => controller,
                None => return Err(ServiceFailure { error : RuntimeError::MissingController,
                                                    report }),
            };
            let mut pending = controller.pending_enabled(core).map_err(|error| {
                ServiceFailure { error : if error == DriverError::InvalidParam {
                                             RuntimeError::InvalidCore
                                         } else {
                                             RuntimeError::Controller(error)
                                         },
                                 report }
            })?;
            while pending != 0 {
                let local = pending.trailing_zeros();
                pending &= !(1 << local);
                let acknowledged = controller.mask_ack_claim(bank, local).map_err(|error| {
                    ServiceFailure { error : RuntimeError::Controller(error), report }
                })?;
                report.masked_sources = report.masked_sources.saturating_add(1);
                match self.domain.dispatch(acknowledged) {
                    Ok(()) => report.handled_sources = report.handled_sources.saturating_add(1),
                    Err(_unhandled) => {
                        report.unhandled_sources = report.unhandled_sources.saturating_add(1)
                    }
                }
            }
        }
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;
    use core::sync::atomic::{AtomicU8, Ordering};

    use super::*;

    const BASE0 : usize = 0x1000;
    const BASE1 : usize = 0x1040;
    const ISR0 : usize = 0x2000;
    const ISR1 : usize = 0x2040;
    const ENABLE_STATUS : usize = 0x24;
    const ENABLE_CLEAR : usize = 0x2c;

    #[derive(Default)]
    struct ModelIo {
        values : Vec<(usize, u32)>,
        writes : Vec<(usize, u32)>,
    }

    impl ModelIo {
        fn with(mut self, address : usize, value : u32) -> Self {
            self.values.push((address, value));
            self
        }
    }

    impl RegisterIo for ModelIo {
        fn read32(&self, address : usize) -> u32 {
            self.values.iter().rev().find(|(candidate, _)| *candidate == address)
                       .map_or(0, |(_, value)| *value)
        }
        fn write32(&mut self, address : usize, value : u32) {
            self.writes.push((address, value));
        }
        fn write8(&mut self, _address : usize, _value : u8) {}
    }

    static VISITED : AtomicU8 = AtomicU8::new(0);

    fn record(acknowledged : crate::irq_domain::AcknowledgedIrq) {
        VISITED.fetch_or(1 << acknowledged.irq().local(), Ordering::Relaxed);
    }

    fn runtime() -> BoardIrqRuntime<ModelIo> {
        let bank0 = LioIntc::new(ModelIo::default()
                                     .with(ISR0, 1 << 2)
                                     .with(BASE0 + ENABLE_STATUS, 1 << 2),
                                 0, BASE0, &[ISR0]).unwrap();
        let bank1 = LioIntc::new(ModelIo::default()
                                     .with(ISR1, (1 << 5) | (1 << 7))
                                     .with(BASE1 + ENABLE_STATUS, (1 << 5) | (1 << 7)),
                                 1, BASE1, &[ISR1]).unwrap();
        BoardIrqRuntime::new([Some(bank0), Some(bank1)],
                             [None, None, Some(0), Some(1), None, None, None, None]).unwrap()
    }

    #[test]
    fn services_multiple_parent_lines_and_keeps_unhandled_masked() {
        VISITED.store(0, Ordering::Relaxed);
        let mut runtime = runtime();
        runtime.register(GlobalIrq::from_bank_local(0, 2).unwrap(), record).unwrap();
        runtime.register(GlobalIrq::from_bank_local(1, 7).unwrap(), record).unwrap();
        let report = runtime.service((1 << 2) | (1 << 3), 0).unwrap();
        assert_eq!(report, ServiceReport { parent_lines : 2,
                                           masked_sources : 3,
                                           handled_sources : 2,
                                           unhandled_sources : 1 });
        assert_eq!(VISITED.load(Ordering::Relaxed), (1 << 2) | (1 << 7));
        let mut controllers = runtime.into_controllers();
        let bank0 = controllers[0].take().unwrap().into_inner();
        let bank1 = controllers[1].take().unwrap().into_inner();
        assert_eq!(bank0.writes,
                   [(BASE0 + ENABLE_CLEAR, 1 << 2)]);
        assert_eq!(bank1.writes,
                   [(BASE1 + ENABLE_CLEAR, 1 << 5),
                    (BASE1 + ENABLE_CLEAR, 1 << 7)]);
    }

    #[test]
    fn partial_report_survives_later_unmapped_parent() {
        let mut runtime = runtime();
        let failure = runtime.service((1 << 2) | (1 << 4), 0).unwrap_err();
        assert_eq!(failure.error, RuntimeError::UnmappedParent);
        assert_eq!(failure.report.masked_sources, 1);
        assert_eq!(failure.report.unhandled_sources, 1);
    }
}

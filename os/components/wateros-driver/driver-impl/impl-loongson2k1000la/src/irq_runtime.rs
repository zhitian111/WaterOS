//! Allocation-free board IRQ service core.
//!
//! Runtime assembly with volatile controllers is deliberately separate and
//! remains `UNVERIFIED_ON_HARDWARE`. This module makes snapshot expansion,
//! mask/ack ordering and token dispatch host-testable.

use api_v0::DriverError;

use crate::{irq_domain::{DomainError, GlobalIrq, IrqDisposition, IrqHandler, LioIntcDomain,
                        MAX_BANKS},
            irq_binding::InterruptBinding,
            liointc::{LioIntc, MAIN_REGISTER_BYTES, MAX_CORES, RegisterIo},
            topology::BoardTopology};
use crate::liointc::Route;

const HWI_LINES : usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutError {
    WrongControllerCount,
    InvalidMainMmio,
    InvalidCoreIsr,
    MissingParentLine,
    InvalidParentLine,
    DuplicateParentLine,
    DuplicateMmio,
    AlreadyPublished,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControllerLayout {
    pub main_base : usize,
    pub core_isr : [Option<usize>; MAX_CORES],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeLayout {
    pub controllers : [ControllerLayout; MAX_BANKS],
    pub parent_banks : [Option<u8>; HWI_LINES],
}

impl RuntimeLayout {
    pub fn compile(topology : &BoardTopology) -> Result<Self, LayoutError> {
        let descriptions = &topology.interrupt_controllers;
        if descriptions.len() != MAX_BANKS { return Err(LayoutError::WrongControllerCount); }
        if descriptions[0].main_mmio.base == descriptions[1].main_mmio.base {
            return Err(LayoutError::DuplicateMmio);
        }
        let mut controllers = [ControllerLayout { main_base : 0,
                                                  core_isr : [None; MAX_CORES] }; MAX_BANKS];
        let mut parent_banks = [None; HWI_LINES];
        for description in descriptions {
            let main = description.main_mmio;
            if main.base == 0 || main.base % 4 != 0 || main.size < MAIN_REGISTER_BYTES {
                return Err(LayoutError::InvalidMainMmio);
            }
            if description.core_isr.is_empty() || description.core_isr.len() > MAX_CORES {
                return Err(LayoutError::InvalidCoreIsr);
            }
            let bank = descriptions.iter()
                                   .filter(|candidate| candidate.main_mmio.base < main.base)
                                   .count();
            let mut core_isr = [None; MAX_CORES];
            for (slot, region) in core_isr.iter_mut().zip(&description.core_isr) {
                if region.base == 0 || region.base % 4 != 0 || region.size < 4 {
                    return Err(LayoutError::InvalidCoreIsr);
                }
                *slot = Some(region.base);
            }
            controllers[bank] = ControllerLayout { main_base : main.base, core_isr };
            let mut has_parent = false;
            for parent in description.parent_interrupts.iter().flatten() {
                if parent.cell_count != 1 || parent.cells[0] >= HWI_LINES as u32 {
                    return Err(LayoutError::InvalidParentLine);
                }
                let line = parent.cells[0] as usize;
                if parent_banks[line].replace(bank as u8).is_some() {
                    return Err(LayoutError::DuplicateParentLine);
                }
                has_parent = true;
            }
            if !has_parent { return Err(LayoutError::MissingParentLine); }
        }
        Ok(Self { controllers, parent_banks })
    }
}

#[derive(Debug, Default)]
pub struct RuntimeLayoutSlot {
    layout : Option<RuntimeLayout>,
}

impl RuntimeLayoutSlot {
    pub const fn new() -> Self { Self { layout : None } }
    pub const fn get(&self) -> Option<&RuntimeLayout> { self.layout.as_ref() }
    pub fn publish(&mut self, layout : RuntimeLayout) -> Result<(), LayoutError> {
        if self.layout.is_some() { return Err(LayoutError::AlreadyPublished); }
        self.layout = Some(layout);
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeError {
    InvalidSnapshot,
    InvalidCore,
    UnmappedParent,
    MissingController,
    Controller(DriverError),
    Domain(DomainError),
    NoConfiguredSources,
    DispositionMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ServiceReport {
    pub parent_lines : u8,
    pub masked_sources : u8,
    pub handled_sources : u8,
    pub unhandled_sources : u8,
    pub rearmed_sources : u8,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ServiceFailure {
    pub error : RuntimeError,
    pub report : ServiceReport,
}

pub trait CpuParentActivator {
    fn enable_parent_lines(&mut self, snapshot : u8) -> Result<(), DriverError>;
}

pub struct TransitionFailure<S> {
    pub error : RuntimeError,
    pub state : S,
}

pub struct DormantRuntime<I> {
    runtime : BoardIrqRuntime<I>,
}

pub struct ConfiguredRuntime<I> {
    runtime : BoardIrqRuntime<I>,
    configured_sources : u64,
}

pub struct LiveRuntime<I> {
    runtime : BoardIrqRuntime<I>,
    configured_sources : u64,
}

pub struct BoardIrqRuntime<I> {
    controllers : [Option<LioIntc<I>>; MAX_BANKS],
    parent_banks : [Option<u8>; HWI_LINES],
    domain : LioIntcDomain,
}

impl<I : RegisterIo> BoardIrqRuntime<I> {
    pub fn assemble<F>(layout : RuntimeLayout, mut make_io : F) -> Result<Self, RuntimeError>
    where F : FnMut(usize, ControllerLayout) -> Result<I, DriverError>
    {
        let mut controllers : [Option<LioIntc<I>>; MAX_BANKS] = [None, None];
        for bank in 0..MAX_BANKS {
            let controller_layout = layout.controllers[bank];
            let mut core_isr = [0; MAX_CORES];
            let mut core_count = 0;
            for address in controller_layout.core_isr.iter().flatten() {
                core_isr[core_count] = *address;
                core_count += 1;
            }
            let io = make_io(bank, controller_layout).map_err(RuntimeError::Controller)?;
            let mut controller = LioIntc::new(io,
                                              bank,
                                              controller_layout.main_base,
                                              &core_isr[..core_count])
                .map_err(RuntimeError::Controller)?;
            controller.mask_all();
            controllers[bank] = Some(controller);
        }
        Self::new(controllers, layout.parent_banks)
    }

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

    pub fn into_dormant(self) -> DormantRuntime<I> { DormantRuntime { runtime : self } }

    pub fn into_controllers(self) -> [Option<LioIntc<I>>; MAX_BANKS] {
        self.controllers
    }

    fn service(&mut self, snapshot : usize, core : usize)
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
                    Ok(disposition) => {
                        report.handled_sources = report.handled_sources.saturating_add(1);
                        match disposition {
                            IrqDisposition::KeepMasked => {}
                            IrqDisposition::Rearm(evidence) => {
                                let expected = GlobalIrq::from_bank_local(bank, local)
                                    .map_err(|error| ServiceFailure {
                                        error : RuntimeError::Domain(error), report
                                    })?;
                                if evidence.irq() != expected {
                                    return Err(ServiceFailure {
                                        error : RuntimeError::DispositionMismatch,
                                        report,
                                    });
                                }
                                controller.enable(local).map_err(|error| ServiceFailure {
                                    error : RuntimeError::Controller(error), report
                                })?;
                                report.rearmed_sources = report.rearmed_sources.saturating_add(1);
                            }
                        }
                    }
                    Err(_unhandled) => {
                        report.unhandled_sources = report.unhandled_sources.saturating_add(1)
                    }
                }
            }
        }
        Ok(report)
    }
}

impl<I : RegisterIo> DormantRuntime<I> {
    pub fn configure(mut self,
                     binding : InterruptBinding,
                     route : Route,
                     handler : IrqHandler)
                     -> Result<ConfiguredRuntime<I>, TransitionFailure<Self>> {
        let irq = binding.global_irq();
        if let Err(error) = self.runtime.register(irq, handler) {
            return Err(TransitionFailure { error, state : self });
        }
        let controller = match self.runtime.controllers[irq.bank()].as_mut() {
            Some(controller) => controller,
            None => {
                let _ = self.runtime.domain.unregister(irq);
                return Err(TransitionFailure { error : RuntimeError::MissingController,
                                               state : self });
            }
        };
        if let Err(failure) = binding.arm(controller, route) {
            let _ = self.runtime.domain.unregister(irq);
            return Err(TransitionFailure { error : RuntimeError::Controller(failure.error),
                                           state : self });
        }
        Ok(ConfiguredRuntime { runtime : self.runtime,
                               configured_sources : 1u64 << irq.raw() })
    }
}

impl<I : RegisterIo> ConfiguredRuntime<I> {
    pub fn configure(mut self,
                     binding : InterruptBinding,
                     route : Route,
                     handler : IrqHandler)
                     -> Result<Self, TransitionFailure<Self>> {
        let irq = binding.global_irq();
        if let Err(error) = self.runtime.register(irq, handler) {
            return Err(TransitionFailure { error, state : self });
        }
        let controller = match self.runtime.controllers[irq.bank()].as_mut() {
            Some(controller) => controller,
            None => {
                let _ = self.runtime.domain.unregister(irq);
                return Err(TransitionFailure { error : RuntimeError::MissingController,
                                               state : self });
            }
        };
        if let Err(failure) = binding.arm(controller, route) {
            let _ = self.runtime.domain.unregister(irq);
            return Err(TransitionFailure { error : RuntimeError::Controller(failure.error),
                                           state : self });
        }
        self.configured_sources |= 1u64 << irq.raw();
        Ok(self)
    }

    pub fn activate<A : CpuParentActivator>(self,
                                            activator : &mut A)
                                            -> Result<LiveRuntime<I>, TransitionFailure<Self>> {
        if self.configured_sources == 0 {
            return Err(TransitionFailure { error : RuntimeError::NoConfiguredSources,
                                           state : self });
        }
        let mut parents = 0u8;
        for (line, bank) in self.runtime.parent_banks.iter().enumerate() {
            if let Some(bank) = bank {
                let bank_mask = if *bank == 0 { u32::MAX as u64 }
                                else { (u32::MAX as u64) << 32 };
                if self.configured_sources & bank_mask != 0 { parents |= 1 << line; }
            }
        }
        if parents == 0 {
            return Err(TransitionFailure { error : RuntimeError::UnmappedParent,
                                           state : self });
        }
        if let Err(error) = activator.enable_parent_lines(parents) {
            return Err(TransitionFailure { error : RuntimeError::Controller(error),
                                           state : self });
        }
        Ok(LiveRuntime { runtime : self.runtime,
                         configured_sources : self.configured_sources })
    }
}

impl<I : RegisterIo> LiveRuntime<I> {
    pub fn configured_sources(&self) -> u64 { self.configured_sources }
    pub fn into_runtime(self) -> BoardIrqRuntime<I> { self.runtime }

    /// Service one snapshot. Sources remain masked after dispatch until a
    /// future device-ack disposition contract permits explicit re-enable.
    pub fn service(&mut self, snapshot : usize, core : usize)
                   -> Result<ServiceReport, ServiceFailure> {
        self.runtime.service(snapshot, core)
    }
}

/// Assemble controllers backed by raw volatile physical MMIO.
///
/// # Safety
/// Every main/core ISR address in `layout` must be mapped, accessible and
/// exclusively owned by this driver. Construction immediately writes
/// ENABLE_CLEAR on both controllers. Register behavior is
/// `UNVERIFIED_ON_HARDWARE` until tested on a 2K1000LA board.
#[cfg(target_arch = "loongarch64")]
pub unsafe fn assemble_volatile(layout : RuntimeLayout)
                                -> Result<BoardIrqRuntime<crate::liointc::VolatileMmio>,
                                          RuntimeError> {
    BoardIrqRuntime::assemble(layout, |_bank, _controller| Ok(crate::liointc::VolatileMmio))
}

#[cfg(test)]
mod tests {
    use alloc::{vec, vec::Vec};
    use core::sync::atomic::{AtomicU8, Ordering};
    use api_v0::MmioRegion;

    use super::*;
    use crate::topology::{InterruptControllerDescription, InterruptSpec};

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

    fn record(acknowledged : crate::irq_domain::AcknowledgedIrq) -> IrqDisposition {
        VISITED.fetch_or(1 << acknowledged.irq().local(), Ordering::Relaxed);
        IrqDisposition::KeepMasked
    }

    fn rearm(acknowledged : crate::irq_domain::AcknowledgedIrq) -> IrqDisposition {
        IrqDisposition::Rearm(crate::irq_domain::DeviceAckedIrq::after_device_clear(
            acknowledged.irq()))
    }

    fn mismatched_rearm(_acknowledged : crate::irq_domain::AcknowledgedIrq)
                        -> IrqDisposition {
        IrqDisposition::Rearm(crate::irq_domain::DeviceAckedIrq::after_device_clear(
            GlobalIrq::from_bank_local(0, 1).unwrap()))
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

    fn description(base : usize, line : u32) -> InterruptControllerDescription {
        let mut parents = core::array::from_fn(|_| None);
        parents[0] = Some(InterruptSpec { parent_phandle : 99,
                                          cells : [line, 0, 0, 0],
                                          cell_count : 1 });
        InterruptControllerDescription {
            phandle : Some(base as u32),
            main_mmio : MmioRegion { base, size : 0x40 },
            core_isr : vec![MmioRegion { base : base - 0x400, size : 8 }],
            interrupt_cells : 2,
            parent_interrupts : parents,
            parent_source_maps : [u32::MAX, 0, 0, 0],
        }
    }

    fn board(descriptions : Vec<InterruptControllerDescription>) -> BoardTopology {
        BoardTopology { uarts : vec![], interrupt_controllers : descriptions,
                        mmc_hosts : vec![], dma_controllers : vec![] }
    }

    #[test]
    fn layout_compilation_is_stable_and_strict() {
        let low = description(0x1fe0_1400, 2);
        let high = description(0x1fe0_1440, 3);
        let ordered = RuntimeLayout::compile(&board(vec![low.clone(), high.clone()])).unwrap();
        let reversed = RuntimeLayout::compile(&board(vec![high, low])).unwrap();
        assert_eq!(ordered, reversed);
        assert_eq!(ordered.controllers[0].main_base, 0x1fe0_1400);
        assert_eq!(ordered.controllers[1].main_base, 0x1fe0_1440);
        assert_eq!(ordered.parent_banks[2], Some(0));
        assert_eq!(ordered.parent_banks[3], Some(1));
        assert_eq!(RuntimeLayout::compile(&board(vec![description(0x1000, 2)])),
                   Err(LayoutError::WrongControllerCount));
        assert_eq!(RuntimeLayout::compile(&board(vec![description(0x1000, 2),
                                                       description(0x1040, 2)])),
                   Err(LayoutError::DuplicateParentLine));
    }

    #[test]
    fn layout_slot_publishes_once_without_overwrite() {
        let layout = RuntimeLayout::compile(&board(vec![description(0x1000, 2),
                                                         description(0x1040, 3)])).unwrap();
        let mut slot = RuntimeLayoutSlot::new();
        assert_eq!(slot.publish(layout), Ok(()));
        let replacement = RuntimeLayout { parent_banks : [None; HWI_LINES], ..layout };
        assert_eq!(slot.publish(replacement), Err(LayoutError::AlreadyPublished));
        assert_eq!(slot.get(), Some(&layout));
    }

    #[test]
    fn assembler_passes_stable_banks_and_masks_every_source() {
        let layout = RuntimeLayout::compile(&board(vec![description(0x1000, 2),
                                                         description(0x1040, 3)])).unwrap();
        let mut seen = Vec::new();
        let runtime = BoardIrqRuntime::assemble(layout, |bank, controller| {
            seen.push((bank, controller.main_base));
            Ok(ModelIo::default())
        }).unwrap();
        assert_eq!(seen, [(0, 0x1000), (1, 0x1040)]);
        let mut controllers = runtime.into_controllers();
        assert_eq!(controllers[0].take().unwrap().into_inner().writes,
                   [(0x1000 + ENABLE_CLEAR, u32::MAX)]);
        assert_eq!(controllers[1].take().unwrap().into_inner().writes,
                   [(0x1040 + ENABLE_CLEAR, u32::MAX)]);
    }

    #[test]
    fn assembler_returns_no_runtime_after_second_bank_factory_failure() {
        let layout = RuntimeLayout::compile(&board(vec![description(0x1000, 2),
                                                         description(0x1040, 3)])).unwrap();
        let mut calls = 0;
        let result = BoardIrqRuntime::assemble(layout, |bank, _controller| {
            calls += 1;
            if bank == 1 { Err(DriverError::IoError) } else { Ok(ModelIo::default()) }
        });
        assert!(matches!(result, Err(RuntimeError::Controller(DriverError::IoError))));
        assert_eq!(calls, 2);
    }

    struct Activator {
        fail : bool,
        calls : Vec<u8>,
    }

    impl CpuParentActivator for Activator {
        fn enable_parent_lines(&mut self, snapshot : u8) -> Result<(), DriverError> {
            self.calls.push(snapshot);
            if self.fail {
                self.fail = false;
                Err(DriverError::IoError)
            } else {
                Ok(())
            }
        }
    }

    fn device_binding(topology : &BoardTopology, provider : u32, local : u32)
                      -> InterruptBinding {
        crate::irq_binding::resolve(topology,
                                    &InterruptSpec { parent_phandle : provider,
                                                     cells : [local, 4, 0, 0],
                                                     cell_count : 2 }).unwrap()
    }

    #[test]
    fn typestate_retries_parent_activation_and_rejects_duplicate_source() {
        let topology = board(vec![description(0x1000, 2), description(0x1040, 3)]);
        let layout = RuntimeLayout::compile(&topology).unwrap();
        let runtime = BoardIrqRuntime::assemble(layout,
                                                |_bank, _controller| Ok(ModelIo::default()))
            .unwrap();
        let binding = device_binding(&topology, 0x1000, 6);
        let configured = runtime.into_dormant()
                                .configure(binding,
                                           Route { core_mask : 1, parent_line : 0 },
                                           record)
                                .unwrap_or_else(|_| panic!("initial configure failed"));
        let duplicate = match configured.configure(binding,
                                                    Route { core_mask : 1, parent_line : 0 },
                                                    record) {
            Ok(_) => panic!("duplicate source configured"),
            Err(failure) => failure,
        };
        assert_eq!(duplicate.error,
                   RuntimeError::Domain(DomainError::AlreadyRegistered));
        let mut activator = Activator { fail : true, calls : Vec::new() };
        let failed = match duplicate.state.activate(&mut activator) {
            Ok(_) => panic!("failed activator produced live runtime"),
            Err(failure) => failure,
        };
        assert_eq!(failed.error, RuntimeError::Controller(DriverError::IoError));
        let live = failed.state.activate(&mut activator)
                               .unwrap_or_else(|_| panic!("activation retry failed"));
        assert_eq!(activator.calls, [1 << 2, 1 << 2]);
        assert_eq!(live.configured_sources(), 1 << 6);
        let mut controllers = live.into_runtime().into_controllers();
        let bank0 = controllers[0].take().unwrap().into_inner();
        assert_eq!(bank0.writes.first(), Some(&(0x1000 + ENABLE_CLEAR, u32::MAX)));
        assert_eq!(bank0.writes.last(), Some(&(0x1000 + 0x28, 1 << 6)));
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
                                           unhandled_sources : 1,
                                           rearmed_sources : 0 });
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

    #[test]
    fn rearm_requires_matching_device_ack_evidence() {
        let mut matching_runtime = runtime();
        matching_runtime.register(GlobalIrq::from_bank_local(0, 2).unwrap(), rearm).unwrap();
        let report = matching_runtime.service(1 << 2, 0).unwrap();
        assert_eq!(report.rearmed_sources, 1);
        let mut controllers = matching_runtime.into_controllers();
        assert_eq!(controllers[0].take().unwrap().into_inner().writes,
                   [(BASE0 + ENABLE_CLEAR, 1 << 2),
                    (BASE0 + 0x28, 1 << 2)]);

        let mut mismatched_runtime = runtime();
        mismatched_runtime.register(GlobalIrq::from_bank_local(0, 2).unwrap(),
                                    mismatched_rearm).unwrap();
        let failure = mismatched_runtime.service(1 << 2, 0).unwrap_err();
        assert_eq!(failure.error, RuntimeError::DispositionMismatch);
        assert_eq!(failure.report.handled_sources, 1);
        assert_eq!(failure.report.rearmed_sources, 0);
        let mut controllers = mismatched_runtime.into_controllers();
        assert_eq!(controllers[0].take().unwrap().into_inner().writes,
                   [(BASE0 + ENABLE_CLEAR, 1 << 2)]);
    }
}

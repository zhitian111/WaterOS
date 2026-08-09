//! Allocation-free board IRQ service core.
//!
//! Runtime assembly with volatile controllers is deliberately separate and
//! remains `UNVERIFIED_ON_HARDWARE`. This module makes snapshot expansion,
//! mask/ack ordering and token dispatch host-testable.

use api_v0::DriverError;

use crate::{irq_domain::{DomainError, GlobalIrq, IrqDisposition, MAX_BANKS},
            irq_binding::InterruptBinding,
            irq_owner::{IrqOwner, IrqOwnerTable, OwnerError},
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
    Owner(OwnerError),
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
    fn disable_parent_lines(&mut self, snapshot : u8) -> Result<(), DriverError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParentActivationReport {
    pub requested : u8,
    pub already_enabled : u8,
    pub newly_enabled : u8,
}

pub struct ActivationFailure<S> {
    pub error : RuntimeError,
    pub source_rollback_error : Option<RuntimeError>,
    pub rollback_error : Option<DriverError>,
    /// Parent inputs that this transaction enabled but could not roll back.
    pub residual_parent_lines : u8,
    pub report : ParentActivationReport,
    pub state : S,
}

pub struct TransitionFailure<S> {
    pub error : RuntimeError,
    pub state : S,
}

pub struct ConfigureFailure<S, O> {
    pub error : RuntimeError,
    pub state : S,
    pub owner : O,
}

pub struct DormantRuntime<I, O> {
    runtime : BoardIrqRuntime<I, O>,
}

pub struct ConfiguredRuntime<I, O> {
    runtime : BoardIrqRuntime<I, O>,
    configured_sources : u64,
}

pub struct LiveRuntime<I, O> {
    runtime : BoardIrqRuntime<I, O>,
    configured_sources : u64,
    parent_lines : u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuiesceReport {
    pub masked_sources : u64,
    pub disabled_parent_lines : u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuiesceError {
    Source(RuntimeError),
    Parent(DriverError),
}

pub struct BoardIrqRuntime<I, O> {
    controllers : [Option<LioIntc<I>>; MAX_BANKS],
    parent_banks : [Option<u8>; HWI_LINES],
    owners : IrqOwnerTable<O>,
}

impl<I : RegisterIo, O> BoardIrqRuntime<I, O> {
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
                  owners : IrqOwnerTable::new() })
    }

    pub fn register(&mut self, irq : GlobalIrq, owner : O)
                    -> Result<(), (RuntimeError, O)> {
        self.owners.register(irq, owner)
                   .map_err(|(error, owner)| (RuntimeError::Owner(error), owner))
    }

    pub fn owner(&self, irq : GlobalIrq) -> Result<&O, RuntimeError> {
        self.owners.get(irq).map_err(RuntimeError::Owner)
    }

    pub fn into_dormant(self) -> DormantRuntime<I, O> { DormantRuntime { runtime : self } }

    pub fn into_controllers(self) -> [Option<LioIntc<I>>; MAX_BANKS] {
        self.controllers
    }

    fn service(&mut self, snapshot : usize, core : usize)
                   -> Result<ServiceReport, ServiceFailure>
    where O : IrqOwner
    {
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
                let active = match self.owners.begin(acknowledged) {
                    Ok(active) => active,
                    Err(failure) if failure.error == OwnerError::NotRegistered => {
                        report.unhandled_sources = report.unhandled_sources.saturating_add(1);
                        continue;
                    }
                    Err(failure) => return Err(ServiceFailure {
                        error : RuntimeError::Owner(failure.error), report
                    }),
                };
                let (active, disposition) = active.handle();
                if let Err(failure) = self.owners.finish(active) {
                    return Err(ServiceFailure { error : RuntimeError::Owner(failure.error),
                                                report });
                }
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
        }
        Ok(report)
    }
}

impl<I : RegisterIo, O> DormantRuntime<I, O> {
    pub fn configure(mut self,
                     binding : InterruptBinding,
                     route : Route,
                     owner : O)
                     -> Result<ConfiguredRuntime<I, O>, ConfigureFailure<Self, O>> {
        let irq = binding.global_irq();
        if let Err((error, owner)) = self.runtime.register(irq, owner) {
            return Err(ConfigureFailure { error, state : self, owner });
        }
        let controller = match self.runtime.controllers[irq.bank()].as_mut() {
            Some(controller) => controller,
            None => {
                let owner = self.runtime.owners.unregister(irq).expect("registered owner missing");
                return Err(ConfigureFailure { error : RuntimeError::MissingController,
                                              state : self,
                                              owner });
            }
        };
        if let Err(failure) = binding.configure_masked(controller, route) {
            let owner = self.runtime.owners.unregister(irq).expect("registered owner missing");
            return Err(ConfigureFailure { error : RuntimeError::Controller(failure.error),
                                          state : self,
                                          owner });
        }
        Ok(ConfiguredRuntime { runtime : self.runtime,
                               configured_sources : 1u64 << irq.raw() })
    }
}

impl<I : RegisterIo, O> ConfiguredRuntime<I, O> {
    pub const fn configured_sources(&self) -> u64 { self.configured_sources }

    pub fn configure(mut self,
                     binding : InterruptBinding,
                     route : Route,
                     owner : O)
                     -> Result<Self, ConfigureFailure<Self, O>> {
        let irq = binding.global_irq();
        if let Err((error, owner)) = self.runtime.register(irq, owner) {
            return Err(ConfigureFailure { error, state : self, owner });
        }
        let controller = match self.runtime.controllers[irq.bank()].as_mut() {
            Some(controller) => controller,
            None => {
                let owner = self.runtime.owners.unregister(irq).expect("registered owner missing");
                return Err(ConfigureFailure { error : RuntimeError::MissingController,
                                              state : self,
                                              owner });
            }
        };
        if let Err(failure) = binding.configure_masked(controller, route) {
            let owner = self.runtime.owners.unregister(irq).expect("registered owner missing");
            return Err(ConfigureFailure { error : RuntimeError::Controller(failure.error),
                                          state : self,
                                          owner });
        }
        self.configured_sources |= 1u64 << irq.raw();
        Ok(self)
    }

    fn required_parent_lines(&self) -> Result<u8, RuntimeError> {
        if self.configured_sources == 0 {
            return Err(RuntimeError::NoConfiguredSources);
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
            return Err(RuntimeError::UnmappedParent);
        }
        Ok(parents)
    }

    fn set_configured_sources(&mut self, enabled : bool) -> Result<(), RuntimeError> {
        let mut remaining = self.configured_sources;
        while remaining != 0 {
            let raw = remaining.trailing_zeros() as usize;
            remaining &= !(1u64 << raw);
            let controller = self.runtime.controllers[raw / 32]
                                         .as_mut()
                                         .ok_or(RuntimeError::MissingController)?;
            if enabled {
                controller.enable((raw % 32) as u32).map_err(RuntimeError::Controller)?;
            } else {
                controller.mask_ack((raw % 32) as u32).map_err(RuntimeError::Controller)?;
            }
        }
        Ok(())
    }

    pub fn activate<A : CpuParentActivator>(self,
                                            activator : &mut A)
                                            -> Result<LiveRuntime<I, O>, TransitionFailure<Self>> {
        self.activate_transactional(activator, 0, |_| Ok(()))
            .map_err(|failure| TransitionFailure { error : failure.error,
                                                   state : failure.state })
    }

    /// Enable the parent inputs owned by this runtime and run a final commit
    /// step.  A failed commit disables only inputs newly enabled here.
    ///
    /// `already_enabled` must be the caller's current-CPU ownership snapshot,
    /// not a raw ECFG value.  Real CSR behavior is `UNVERIFIED_ON_HARDWARE`.
    pub fn activate_transactional<A, F>(mut self,
                                        activator : &mut A,
                                        already_enabled : u8,
                                        mut commit : F)
                                        -> Result<LiveRuntime<I, O>, ActivationFailure<Self>>
    where A : CpuParentActivator,
          F : FnMut(ParentActivationReport) -> Result<(), DriverError>
    {
        let requested = match self.required_parent_lines() {
            Ok(parents) => parents,
            Err(error) => return Err(ActivationFailure {
                error,
                source_rollback_error : None,
                rollback_error : None,
                residual_parent_lines : 0,
                report : ParentActivationReport { requested : 0,
                                                  already_enabled,
                                                  newly_enabled : 0 },
                state : self,
            }),
        };
        let report = ParentActivationReport {
            requested,
            already_enabled,
            newly_enabled : requested & !already_enabled,
        };
        if report.newly_enabled != 0 {
            if let Err(error) = activator.enable_parent_lines(report.newly_enabled) {
                return Err(ActivationFailure {
                    error : RuntimeError::Controller(error),
                    source_rollback_error : None,
                    rollback_error : None,
                    residual_parent_lines : 0,
                    report,
                    state : self,
                });
            }
        }
        if let Err(error) = self.set_configured_sources(true) {
            let rollback_error = if report.newly_enabled == 0 {
                None
            } else {
                activator.disable_parent_lines(report.newly_enabled).err()
            };
            return Err(ActivationFailure {
                error,
                source_rollback_error : self.set_configured_sources(false).err(),
                rollback_error,
                residual_parent_lines : if rollback_error.is_some() {
                    report.newly_enabled
                } else {
                    0
                },
                report,
                state : self,
            });
        }
        if let Err(error) = commit(report) {
            let source_rollback_error = self.set_configured_sources(false).err();
            let rollback_error = if report.newly_enabled == 0 {
                None
            } else {
                activator.disable_parent_lines(report.newly_enabled).err()
            };
            return Err(ActivationFailure {
                error : RuntimeError::Controller(error),
                source_rollback_error,
                rollback_error,
                residual_parent_lines : if rollback_error.is_some() {
                    report.newly_enabled
                } else {
                    0
                },
                report,
                state : self,
            });
        }
        Ok(LiveRuntime { runtime : self.runtime,
                         configured_sources : self.configured_sources,
                         parent_lines : requested })
    }
}

impl<I : RegisterIo, O : IrqOwner> LiveRuntime<I, O> {
    pub fn configured_sources(&self) -> u64 { self.configured_sources }
    pub fn parent_lines(&self) -> u8 { self.parent_lines }
    pub fn into_runtime(self) -> BoardIrqRuntime<I, O> { self.runtime }

    /// Service one snapshot. Sources remain masked after dispatch until a
    /// future device-ack disposition contract permits explicit re-enable.
    pub fn service(&mut self, snapshot : usize, core : usize)
                   -> Result<ServiceReport, ServiceFailure> {
        self.runtime.service(snapshot, core)
    }

    /// Stop device delivery before disabling this runtime's CPU parent lines.
    /// Failure leaves the runtime owned by the caller for a retry.
    pub fn quiesce<A : CpuParentActivator>(&mut self,
                                           activator : &mut A)
                                           -> Result<QuiesceReport, QuiesceError> {
        let mut remaining = self.configured_sources;
        while remaining != 0 {
            let raw = remaining.trailing_zeros() as usize;
            remaining &= !(1u64 << raw);
            let controller = self.runtime.controllers[raw / 32]
                                         .as_mut()
                                         .ok_or(QuiesceError::Source(
                                             RuntimeError::MissingController))?;
            controller.mask_ack((raw % 32) as u32)
                      .map_err(|error| QuiesceError::Source(
                          RuntimeError::Controller(error)))?;
        }
        let parents = self.parent_lines;
        if parents != 0 {
            activator.disable_parent_lines(parents).map_err(QuiesceError::Parent)?;
            self.parent_lines = 0;
        }
        Ok(QuiesceReport { masked_sources : self.configured_sources,
                           disabled_parent_lines : parents })
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
pub unsafe fn assemble_volatile<O>(layout : RuntimeLayout)
                                -> Result<BoardIrqRuntime<crate::liointc::VolatileMmio, O>,
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

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum OwnerMode {
        KeepMasked,
        Rearm,
        MismatchedRearm,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct TestOwner {
        mode : OwnerMode,
        handled : u8,
    }

    impl TestOwner {
        const fn keep() -> Self { Self { mode : OwnerMode::KeepMasked, handled : 0 } }
        const fn rearm() -> Self { Self { mode : OwnerMode::Rearm, handled : 0 } }
        const fn mismatched() -> Self {
            Self { mode : OwnerMode::MismatchedRearm, handled : 0 }
        }
    }

    impl IrqOwner for TestOwner {
        fn handle(&mut self,
                  acknowledged : crate::irq_domain::AcknowledgedIrq)
                  -> IrqDisposition {
            self.handled += 1;
            VISITED.fetch_or(1 << acknowledged.irq().local(), Ordering::Relaxed);
            match self.mode {
                OwnerMode::KeepMasked => IrqDisposition::KeepMasked,
                OwnerMode::Rearm => IrqDisposition::Rearm(
                    crate::irq_domain::DeviceAckedIrq::after_device_clear(acknowledged.irq())),
                OwnerMode::MismatchedRearm => IrqDisposition::Rearm(
                    crate::irq_domain::DeviceAckedIrq::after_device_clear(
                        GlobalIrq::from_bank_local(0, 1).unwrap())),
            }
        }
    }

    fn runtime() -> BoardIrqRuntime<ModelIo, TestOwner> {
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
        let runtime : BoardIrqRuntime<ModelIo, TestOwner> =
            BoardIrqRuntime::assemble(layout, |bank, controller| {
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
        let result : Result<BoardIrqRuntime<ModelIo, TestOwner>, RuntimeError> =
            BoardIrqRuntime::assemble(layout, |bank, _controller| {
            calls += 1;
            if bank == 1 { Err(DriverError::IoError) } else { Ok(ModelIo::default()) }
            });
        assert!(matches!(result, Err(RuntimeError::Controller(DriverError::IoError))));
        assert_eq!(calls, 2);
    }

    struct Activator {
        fail_enable : bool,
        fail_disable : bool,
        enables : Vec<u8>,
        disables : Vec<u8>,
    }

    impl CpuParentActivator for Activator {
        fn enable_parent_lines(&mut self, snapshot : u8) -> Result<(), DriverError> {
            self.enables.push(snapshot);
            if self.fail_enable {
                self.fail_enable = false;
                Err(DriverError::IoError)
            } else {
                Ok(())
            }
        }

        fn disable_parent_lines(&mut self, snapshot : u8) -> Result<(), DriverError> {
            self.disables.push(snapshot);
            if self.fail_disable {
                self.fail_disable = false;
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
        let runtime : BoardIrqRuntime<ModelIo, TestOwner> =
            BoardIrqRuntime::assemble(layout,
                                      |_bank, _controller| Ok(ModelIo::default()))
            .unwrap();
        let binding = device_binding(&topology, 0x1000, 6);
        let configured = runtime.into_dormant()
                                .configure(binding,
                                           Route { core_mask : 1, parent_line : 0 },
                                           TestOwner::keep())
                                .unwrap_or_else(|_| panic!("initial configure failed"));
        let duplicate = match configured.configure(binding,
                                                    Route { core_mask : 1, parent_line : 0 },
                                                    TestOwner::keep()) {
            Ok(_) => panic!("duplicate source configured"),
            Err(failure) => failure,
        };
        assert_eq!(duplicate.error,
                   RuntimeError::Owner(OwnerError::AlreadyRegistered));
        assert_eq!(duplicate.owner, TestOwner::keep());
        let mut activator = Activator { fail_enable : true,
                                        fail_disable : false,
                                        enables : Vec::new(),
                                        disables : Vec::new() };
        let failed = match duplicate.state.activate(&mut activator) {
            Ok(_) => panic!("failed activator produced live runtime"),
            Err(failure) => failure,
        };
        assert_eq!(failed.error, RuntimeError::Controller(DriverError::IoError));
        let live = failed.state.activate(&mut activator)
                               .unwrap_or_else(|_| panic!("activation retry failed"));
        assert_eq!(activator.enables, [1 << 2, 1 << 2]);
        assert!(activator.disables.is_empty());
        assert_eq!(live.configured_sources(), 1 << 6);
        assert_eq!(live.parent_lines(), 1 << 2);
        let mut controllers = live.into_runtime().into_controllers();
        let bank0 = controllers[0].take().unwrap().into_inner();
        assert_eq!(bank0.writes.first(), Some(&(0x1000 + ENABLE_CLEAR, u32::MAX)));
        assert_eq!(bank0.writes.last(), Some(&(0x1000 + 0x28, 1 << 6)));
    }

    #[test]
    fn activation_rolls_back_only_new_parent_lines_and_reports_residue() {
        let topology = board(vec![description(0x1000, 2), description(0x1040, 3)]);
        let layout = RuntimeLayout::compile(&topology).unwrap();
        let runtime : BoardIrqRuntime<ModelIo, TestOwner> =
            BoardIrqRuntime::assemble(layout,
                                      |_bank, _controller| Ok(ModelIo::default()))
            .unwrap();
        let configured = runtime.into_dormant()
                                .configure(device_binding(&topology, 0x1000, 6),
                                           Route { core_mask : 1, parent_line : 0 },
                                           TestOwner::keep())
                                .unwrap_or_else(|_| panic!("configure failed"));
        let mut activator = Activator { fail_enable : false,
                                        fail_disable : true,
                                        enables : Vec::new(),
                                        disables : Vec::new() };
        let failure = match configured.activate_transactional(&mut activator,
                                                               1 << 3,
                                                               |_| Err(DriverError::InvalidParam)) {
            Ok(_) => panic!("failed commit produced live runtime"),
            Err(failure) => failure,
        };
        assert_eq!(failure.error, RuntimeError::Controller(DriverError::InvalidParam));
        assert_eq!(failure.rollback_error, Some(DriverError::IoError));
        assert_eq!(failure.residual_parent_lines, 1 << 2);
        assert_eq!(failure.report, ParentActivationReport { requested : 1 << 2,
                                                            already_enabled : 1 << 3,
                                                            newly_enabled : 1 << 2 });
        assert_eq!(activator.enables, [1 << 2]);
        assert_eq!(activator.disables, [1 << 2]);

        let live = failure.state
                          .activate_transactional(&mut activator,
                                                  (1 << 2) | (1 << 3),
                                                  |_| Ok(()))
                          .unwrap_or_else(|_| panic!("retry failed"));
        assert_eq!(live.parent_lines(), 1 << 2);
        assert_eq!(activator.enables, [1 << 2]);
        assert_eq!(activator.disables, [1 << 2]);
        let mut controllers = live.into_runtime().into_controllers();
        let writes = controllers[0].take().unwrap().into_inner().writes;
        assert_eq!(&writes[writes.len() - 3..],
                   [(BASE0 + 0x28, 1 << 6),
                    (BASE0 + ENABLE_CLEAR, 1 << 6),
                    (BASE0 + 0x28, 1 << 6)]);
    }

    #[test]
    fn quiesce_masks_sources_before_parent_disable_and_retries_failure() {
        let topology = board(vec![description(0x1000, 2), description(0x1040, 3)]);
        let layout = RuntimeLayout::compile(&topology).unwrap();
        let runtime : BoardIrqRuntime<ModelIo, TestOwner> =
            BoardIrqRuntime::assemble(layout,
                                      |_bank, _controller| Ok(ModelIo::default()))
            .unwrap();
        let configured = runtime.into_dormant()
                                .configure(device_binding(&topology, 0x1000, 6),
                                           Route { core_mask : 1, parent_line : 0 },
                                           TestOwner::keep())
                                .unwrap_or_else(|_| panic!("configure failed"));
        let mut activator = Activator { fail_enable : false,
                                        fail_disable : false,
                                        enables : Vec::new(),
                                        disables : Vec::new() };
        let mut live = configured.activate(&mut activator)
                                 .unwrap_or_else(|_| panic!("activate failed"));
        activator.fail_disable = true;
        assert_eq!(live.quiesce(&mut activator),
                   Err(QuiesceError::Parent(DriverError::IoError)));
        assert_eq!(live.parent_lines(), 1 << 2);
        assert_eq!(live.quiesce(&mut activator),
                   Ok(QuiesceReport { masked_sources : 1 << 6,
                                      disabled_parent_lines : 1 << 2 }));
        assert_eq!(live.parent_lines(), 0);
        assert_eq!(activator.disables, [1 << 2, 1 << 2]);
        let mut controllers = live.into_runtime().into_controllers();
        let writes = controllers[0].take().unwrap().into_inner().writes;
        assert_eq!(&writes[writes.len() - 3..],
                   [(BASE0 + 0x28, 1 << 6),
                    (BASE0 + ENABLE_CLEAR, 1 << 6),
                    (BASE0 + ENABLE_CLEAR, 1 << 6)]);
    }

    #[test]
    fn services_multiple_parent_lines_and_keeps_unhandled_masked() {
        VISITED.store(0, Ordering::Relaxed);
        let mut runtime = runtime();
        runtime.register(GlobalIrq::from_bank_local(0, 2).unwrap(), TestOwner::keep()).unwrap();
        runtime.register(GlobalIrq::from_bank_local(1, 7).unwrap(), TestOwner::keep()).unwrap();
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
        matching_runtime.register(GlobalIrq::from_bank_local(0, 2).unwrap(),
                                  TestOwner::rearm()).unwrap();
        let report = matching_runtime.service(1 << 2, 0).unwrap();
        assert_eq!(report.rearmed_sources, 1);
        let report = matching_runtime.service(1 << 2, 0).unwrap();
        assert_eq!(report.rearmed_sources, 1);
        assert_eq!(matching_runtime.owner(GlobalIrq::from_bank_local(0, 2).unwrap())
                                   .unwrap()
                                   .handled,
                   2);
        let mut controllers = matching_runtime.into_controllers();
        assert_eq!(controllers[0].take().unwrap().into_inner().writes,
                   [(BASE0 + ENABLE_CLEAR, 1 << 2),
                    (BASE0 + 0x28, 1 << 2),
                    (BASE0 + ENABLE_CLEAR, 1 << 2),
                    (BASE0 + 0x28, 1 << 2)]);

        let mut mismatched_runtime = runtime();
        mismatched_runtime.register(GlobalIrq::from_bank_local(0, 2).unwrap(),
                                    TestOwner::mismatched()).unwrap();
        let failure = mismatched_runtime.service(1 << 2, 0).unwrap_err();
        assert_eq!(failure.error, RuntimeError::DispositionMismatch);
        assert_eq!(failure.report.handled_sources, 1);
        assert_eq!(failure.report.rearmed_sources, 0);
        let mut controllers = mismatched_runtime.into_controllers();
        assert_eq!(controllers[0].take().unwrap().into_inner().writes,
                   [(BASE0 + ENABLE_CLEAR, 1 << 2)]);
    }
}

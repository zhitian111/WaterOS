//! Loongson 2K1000LA DTB-first machine driver.
//!
//! This profile currently discovers and validates board resources only. It
//! deliberately performs no LIOINTC, MMC clock/reset or DMA register writes:
//! those activation paths are `UNVERIFIED_ON_HARDWARE` until a board is
//! available.

#![no_std]
extern crate alloc;

pub mod irq_domain;
pub mod irq_binding;
pub mod irq_entry;
pub mod irq_runtime;
pub mod irq_owner;
pub mod board_irq_owner;
pub mod clock;
pub mod irq_plan;
pub mod cpu_parent;
pub mod diagnostic_slot;
pub mod diagnostic_irq;
pub mod liointc;
pub mod mmc;
pub mod mmc_diagnostic;
pub mod mmc_prerequisite;
pub mod pinctrl;
pub mod apbdma;
pub mod apbdma_mmio;
pub mod dma_memory;
pub mod gpio;
mod machine;
pub mod topology;

use api_v0::{DriverError, DriverResult};
use spin::Mutex;
use irq_runtime::{RuntimeLayout, RuntimeLayoutSlot};
use irq_plan::BoardOwnerPlan;
use topology::BoardTopology;

static TOPOLOGY : Mutex<Option<BoardTopology>> = Mutex::new(None);
static IRQ_LAYOUT : Mutex<RuntimeLayoutSlot> = Mutex::new(RuntimeLayoutSlot::new());
static IRQ_OWNER_PLAN : Mutex<Option<BoardOwnerPlan>> = Mutex::new(None);
#[cfg(target_arch = "loongarch64")]
static MMC_DIAGNOSTIC_GATE : mmc_diagnostic::DiagnosticGate =
    mmc_diagnostic::DiagnosticGate::new();

#[cfg(target_arch = "loongarch64")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmcDiagnosticError {
    Busy,
    TopologyUnavailable,
    HostCount,
    Diagnosis(mmc_diagnostic::VolatileDiagnosisError),
}

pub use machine::machine;

#[cfg(target_arch = "loongarch64")]
pub fn init_after_boot() -> DriverResult<()> {
    let fdt = common::dtb::read_fdt(platform::dtb_pa())?;
    let topology = topology::discover(&fdt)?;
    let irq_layout = RuntimeLayout::compile(&topology).map_err(|error| {
        log::error!("[driver-ls2k][irq] invalid runtime layout: {:?}", error);
        DriverError::InvalidDtb
    })?;
    let owner_plan = irq_plan::compile(&topology).map_err(|error| {
        log::error!("[driver-ls2k][irq] invalid owner plan: {:?}", error);
        DriverError::InvalidDtb
    })?;
    log::info!("[driver-ls2k] topology: uart={} liointc={} dma={} mmc={}",
               topology.uarts.len(),
               topology.interrupt_controllers
                       .len(),
               topology.dma_controllers
                       .len(),
               topology.mmc_hosts
                       .len());
    for host in &topology.mmc_hosts {
        let plan = mmc::plan(host).map_err(|error| {
                                      log::error!("[driver-ls2k][mmc] invalid deferred plan: {:?}",
                                                  error);
                                      DriverError::InvalidDtb
                                  })?;
        log::warn!("[driver-ls2k][mmc] deferred controller={:#x}/{:#x} \
                    auxiliary={:#x}/{:#x} bus_width={} prerequisites={:?} blockers={:?}; \
                    hardware activation=UNVERIFIED_ON_HARDWARE",
                   plan.controller_mmio.base,
                   plan.controller_mmio.size,
                   plan.auxiliary_mmio.base,
                   plan.auxiliary_mmio.size,
                   plan.bus_width,
                   plan.prerequisites,
                   plan.blockers);
    }
    let mut stored_topology = TOPOLOGY.lock();
    let mut stored_layout = IRQ_LAYOUT.lock();
    let mut stored_owner_plan = IRQ_OWNER_PLAN.lock();
    if stored_topology.is_some() || stored_layout.get().is_some() || stored_owner_plan.is_some() {
        return Err(DriverError::InvalidParam);
    }
    stored_layout.publish(irq_layout).map_err(|_| DriverError::InvalidParam)?;
    *stored_owner_plan = Some(owner_plan);
    *stored_topology = Some(topology);
    log::warn!("[driver-ls2k] resources discovered but hardware activation is \
                UNVERIFIED_ON_HARDWARE");
    Ok(())
}

#[cfg(not(target_arch = "loongarch64"))]
pub fn init_after_boot() -> DriverResult<()> { Err(DriverError::Unsupported) }

pub fn with_topology<R>(f : impl FnOnce(Option<&BoardTopology>) -> R) -> R {
    let topology = TOPOLOGY.lock();
    f(topology.as_ref())
}

/// Perform one explicitly requested read-only MMC prerequisite diagnosis.
///
/// # Safety
/// The topology MMIO windows must be mapped device memory and no other owner
/// may mutate the clock/GPIO registers concurrently. Physical behavior is
/// `UNVERIFIED_ON_HARDWARE`; this function never activates the MMC host.
#[cfg(target_arch = "loongarch64")]
pub unsafe fn diagnose_mmc_once()
    -> Result<mmc_diagnostic::VolatileDiagnosis, MmcDiagnosticError> {
    let _guard = MMC_DIAGNOSTIC_GATE.try_enter().map_err(|_| MmcDiagnosticError::Busy)?;
    let description = with_topology(|topology| {
        let topology = topology.ok_or(MmcDiagnosticError::TopologyUnavailable)?;
        if topology.mmc_hosts.len() != 1 {
            return Err(MmcDiagnosticError::HostCount);
        }
        Ok(topology.mmc_hosts[0].clone())
    })?;
    // SAFETY: forwarded to this function's caller; the gate excludes another
    // diagnosis and the topology lock has already been released.
    unsafe { mmc_diagnostic::diagnose_volatile(&description) }
        .map_err(MmcDiagnosticError::Diagnosis)
}

pub fn with_irq_layout<R>(f : impl FnOnce(Option<&RuntimeLayout>) -> R) -> R {
    let layout = IRQ_LAYOUT.lock();
    f(layout.get())
}

pub fn with_irq_owner_plan<R>(f : impl FnOnce(Option<&BoardOwnerPlan>) -> R) -> R {
    let plan = IRQ_OWNER_PLAN.lock();
    f(plan.as_ref())
}

fn self_test() {
    if with_topology(|topology| topology.is_none()) {
        log::info!("[driver-ls2k] topology not initialized; skip runtime self-test");
    }
}

fn unsupported_realtime() -> DriverResult<Option<u64>> { Err(DriverError::Unsupported) }

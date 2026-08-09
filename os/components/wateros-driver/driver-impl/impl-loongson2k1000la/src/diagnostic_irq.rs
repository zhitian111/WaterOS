//! Explicit, opt-in real-hardware IRQ diagnostic runtime.
//!
//! Safe-default boot never calls this module's activation entry point.

use api_v0::{DriverError, DriverResult};

use crate::diagnostic_slot::SlotError;

#[cfg(target_arch = "loongarch64")]
use crate::diagnostic_slot::DrainError;

#[cfg(target_arch = "loongarch64")]
use crate::{
    board_irq_owner::{BoardIrqOwner, MmcCommandOwner},
    cpu_parent::LoongArchCpuParentActivator,
    diagnostic_slot::DiagnosticRuntimeSlot,
    irq_plan::{ApplyError, ApplyMode, AppliedRuntime, OwnerKind},
    irq_runtime::{LiveRuntime, QuiesceError, RuntimeError},
    liointc::VolatileMmio,
    mmc::VolatileRegisters,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticIrqError {
    NotInitialized,
    Slot(SlotError),
    #[cfg(target_arch = "loongarch64")]
    Assemble(RuntimeError),
    #[cfg(target_arch = "loongarch64")]
    Apply(ApplyError),
    UnexpectedDormant,
    #[cfg(target_arch = "loongarch64")]
    Activate {
        error : RuntimeError,
        source_rollback : Option<RuntimeError>,
        parent_rollback : Option<DriverError>,
        residual_parent_lines : u8,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticDrainError {
    NotInitialized,
    Slot(SlotError),
    #[cfg(target_arch = "loongarch64")]
    Quiesce(QuiesceError),
}

#[cfg(target_arch = "loongarch64")]
type TargetRuntime = LiveRuntime<VolatileMmio, BoardIrqOwner<VolatileRegisters>>;

#[cfg(target_arch = "loongarch64")]
static RUNTIME : DiagnosticRuntimeSlot<TargetRuntime> = DiagnosticRuntimeSlot::new();

/// Assemble and publish the 2K1000 diagnostic IRQ runtime once.
///
/// # Safety
///
/// The topology MMIO regions must be mapped as device memory and exclusively
/// owned by this driver.  The caller must run on boot CPU 0 after
/// `init_after_boot`, with no other LIOINTC/ECFG owner.  Physical interrupt
/// delivery and register semantics remain `UNVERIFIED_ON_HARDWARE`.
#[cfg(target_arch = "loongarch64")]
pub unsafe fn activate() -> Result<(), DiagnosticIrqError> {
    let reservation = RUNTIME.reserve().map_err(DiagnosticIrqError::Slot)?;
    let layout = crate::with_irq_layout(|layout| layout.copied())
        .ok_or(DiagnosticIrqError::NotInitialized)?;
    let plan = crate::with_irq_owner_plan(|plan| plan.copied())
        .ok_or(DiagnosticIrqError::NotInitialized)?;

    // SAFETY: upheld by this function's caller contract. Assembly immediately
    // masks both LIOINTC banks before any source-specific configuration.
    let runtime = unsafe { crate::irq_runtime::assemble_volatile(layout) }
        .map_err(DiagnosticIrqError::Assemble)?
        .into_dormant();
    let (applied, report) = crate::irq_plan::apply_owner_plan(
        runtime,
        &plan,
        ApplyMode::DiagnosticAckOnly,
        |entry| match entry.kind {
            OwnerKind::MmcCommand => {
                // SAFETY: entry.device_mmio was topology-validated and is
                // covered by this function's exclusive ownership contract.
                let registers = unsafe { VolatileRegisters::from_region(entry.device_mmio) }
                    .map_err(|_| DriverError::IoError)?;
                Ok(BoardIrqOwner::MmcCommand(MmcCommandOwner::new(
                    entry.binding.global_irq(),
                    registers,
                )))
            }
            OwnerKind::ApbDmaDeferred => Err(DriverError::Unsupported),
        },
    )
    .map_err(|failure| DiagnosticIrqError::Apply(failure.error))?;
    let configured = match applied {
        AppliedRuntime::Configured(runtime) => runtime,
        AppliedRuntime::Dormant(_) => return Err(DiagnosticIrqError::UnexpectedDormant),
    };
    debug_assert_eq!(report.configured, 1);

    let mut activator = LoongArchCpuParentActivator;
    let live = configured
        .activate_transactional(&mut activator, 0, |_| Ok(()))
        .map_err(|failure| {
            log::error!("[driver-ls2k][irq] activation status polls: {:?}",
                        failure.state.status_poll_failures());
            DiagnosticIrqError::Activate {
                error : failure.error,
                source_rollback : failure.source_rollback_error,
                parent_rollback : failure.rollback_error,
                residual_parent_lines : failure.residual_parent_lines,
            }
        })?;
    reservation.commit(live);
    log::warn!(
        "[driver-ls2k][irq] diagnostic runtime live; hardware path=UNVERIFIED_ON_HARDWARE"
    );
    Ok(())
}

/// Quiesce and remove the published diagnostic runtime.
///
/// # Safety
/// Must run on the same boot CPU that activated the runtime. Physical ECFG
/// and LIOINTC shutdown remains `UNVERIFIED_ON_HARDWARE`.
#[cfg(target_arch = "loongarch64")]
pub unsafe fn drain() -> Result<(), DiagnosticDrainError> {
    let mut activator = LoongArchCpuParentActivator;
    RUNTIME
        .drain(|runtime| {
            runtime.quiesce(&mut activator).map(|_| ()).map_err(|error| {
                log::error!("[driver-ls2k][irq] drain status polls: {:?}",
                            runtime.status_poll_failures());
                error
            })
        })
        .map_err(|error| match error {
            DrainError::Slot(error) => DiagnosticDrainError::Slot(error),
            DrainError::Operation(error) => DiagnosticDrainError::Quiesce(error),
        })?;
    log::warn!("[driver-ls2k][irq] diagnostic runtime drained");
    Ok(())
}

#[cfg(not(target_arch = "loongarch64"))]
pub unsafe fn activate() -> Result<(), DiagnosticIrqError> {
    Err(DiagnosticIrqError::NotInitialized)
}

#[cfg(not(target_arch = "loongarch64"))]
pub unsafe fn drain() -> Result<(), DiagnosticDrainError> {
    Err(DiagnosticDrainError::NotInitialized)
}

#[cfg(target_arch = "loongarch64")]
pub fn service(snapshot : usize, core : usize) -> DriverResult<()> {
    let result = RUNTIME.with_live_mut(|runtime| runtime.service(snapshot, core));
    match result {
        Ok(Ok(_report)) => Ok(()),
        Ok(Err(failure)) => {
            log::error!("[driver-ls2k][irq] service failure: {:?}", failure.error);
            let _ = RUNTIME.with_live_mut(|runtime| {
                log::error!("[driver-ls2k][irq] service status polls: {:?}",
                            runtime.status_poll_failures());
            });
            Err(DriverError::IoError)
        }
        Err(SlotError::Empty) => Err(DriverError::Unsupported),
        Err(_) => Err(DriverError::IoError),
    }
}

#[cfg(not(target_arch = "loongarch64"))]
pub fn service(_snapshot : usize, _core : usize) -> DriverResult<()> {
    Err(DriverError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_cannot_activate_or_service_real_hardware_runtime() {
        // SAFETY: the host implementation performs no hardware access.
        assert_eq!(unsafe { activate() }, Err(DiagnosticIrqError::NotInitialized));
        // SAFETY: the host implementation performs no hardware access.
        assert_eq!(unsafe { drain() }, Err(DiagnosticDrainError::NotInitialized));
        assert_eq!(service(1 << 2, 0), Err(DriverError::Unsupported));
    }
}

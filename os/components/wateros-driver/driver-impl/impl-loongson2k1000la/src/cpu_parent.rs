//! LoongArch HWI parent-line adapter for the board IRQ runtime.
//!
//! Constructing this adapter is inert.  Calling it writes the current CPU's
//! ECFG CSR and therefore remains outside safe-default initialization.

use api_v0::{DriverError, DriverResult};

use crate::irq_runtime::CpuParentActivator;

/// Activates LoongArch HWI0..HWI7 inputs on the current CPU.
pub struct LoongArchCpuParentActivator;

#[cfg(target_arch = "loongarch64")]
impl CpuParentActivator for LoongArchCpuParentActivator {
    fn enable_parent_lines(&mut self, snapshot: u8) -> DriverResult<()> {
        // UNVERIFIED_ON_HARDWARE: do not call from safe-default boot until the
        // LIOINTC route and handler ownership are ready on a physical 2K1000.
        platform::arch::interrupt::enable_external_interrupt_lines(
            platform::arch::interrupt::ArchExternalInterruptLines(snapshot),
        )
        .map_err(|_| DriverError::IoError)
    }
}

#[cfg(not(target_arch = "loongarch64"))]
impl CpuParentActivator for LoongArchCpuParentActivator {
    fn enable_parent_lines(&mut self, _snapshot: u8) -> DriverResult<()> {
        Err(DriverError::Unsupported)
    }
}

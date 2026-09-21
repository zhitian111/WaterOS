//! External device IRQ controller, routed exclusively to the boot CPU.
//!
//! Initialize once on the BSP after MMIO mappings and trap vectors are installed,
//! while local interrupts remain masked. Registration/unmasking must finish before
//! enabling `arch::interrupt::enable_external_interrupt`. Enable/disable require
//! BSP execution with local interrupts masked; they update controller bitmaps.
//! Claim and completion are lock-free hard-IRQ operations on that same CPU.
//! Drivers must acknowledge their device before completing its controller IRQ.
//! RISC-V IDs are PLIC sources; LoongArch IDs are PCH PIC pins (without GSI +64).

pub use crate::active_impl::irq::{claim, complete, disable, enable, init};

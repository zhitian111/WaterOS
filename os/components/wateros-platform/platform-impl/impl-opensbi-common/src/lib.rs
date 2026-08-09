#![no_std]

//! Board-neutral OpenSBI transports shared by RISC-V machine profiles.
//!
//! This crate contains firmware ABI operations only. UART addresses, memory
//! maps, linker addresses and DTB interpretation remain owned by each board.

pub mod reset;
pub mod smp;
pub mod timer;

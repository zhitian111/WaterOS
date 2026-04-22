#![no_std]

use core::arch::global_asm;

global_asm!(include_str!("../asm/trap.asm"));
global_asm!(include_str!("../asm/switch.S"));

pub mod trap;
pub mod time;
pub mod interrupt;
pub mod task;
pub use trap::init_trap;

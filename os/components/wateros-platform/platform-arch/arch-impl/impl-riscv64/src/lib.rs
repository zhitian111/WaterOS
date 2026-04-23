#![no_std]

use core::arch::global_asm;

global_asm!(include_str!("../asm/trap.asm"));

pub mod trap;
pub mod time;
pub mod interrupt;
pub mod paging;
pub use trap::init_trap;

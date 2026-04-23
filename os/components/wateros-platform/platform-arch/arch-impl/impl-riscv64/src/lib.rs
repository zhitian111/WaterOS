#![no_std]

use core::arch::global_asm;

global_asm!(include_str!("../asm/trap.asm"));
global_asm!(include_str!("../asm/switch.S"));

pub mod interrupt;
pub mod paging;
pub mod task;
pub mod time;
pub mod trap;
pub use trap::init_trap;

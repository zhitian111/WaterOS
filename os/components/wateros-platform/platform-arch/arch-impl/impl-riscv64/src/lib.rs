#![no_std]

use core::arch::global_asm;

global_asm!(include_str!("../asm/trap.asm"));
global_asm!(include_str!("../asm/switch.S"));

pub mod trap;
pub mod time;
pub mod interrupt;
<<<<<<< HEAD
pub mod paging;
=======
pub mod task;
>>>>>>> dad8b4edb02022576e3eabb0ef766e276ce0eb6f
pub use trap::init_trap;

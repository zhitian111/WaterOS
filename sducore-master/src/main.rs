#![no_std]
#![no_main]

use core::arch::global_asm;

//#![feature(global_asm)]

// do this so we do not need assembler
global_asm!(include_str!("entry.asm"));

pub const KERNEL_BASE : usize = 0xFFFF_FFC0_0000_0000;

use core::panic::PanicInfo;

#[unsafe(no_mangle)]
pub extern "C" fn rust_main(_hart_id : usize) {
    loop {}
}

#[panic_handler]
fn panic_handler(_info : &PanicInfo) -> ! {
    loop {} // Enter an infinite loop on panic
}

#![no_std]
#![no_main]
use core::arch::asm;
use core::panic::PanicInfo;
use water_os::io::stdout::{prints, uart_init};
use water_os::print;

pub const KERNEL_BASE : usize = 0xFFFF_FFC0_0000_0000;

pub const USER_STACK_SIZE : usize = 0x0000_0040;
static mut USER_STACK : [u8; USER_STACK_SIZE] = [0; USER_STACK_SIZE];

pub fn get_user_stack_top_ptr() -> *mut u8 {
    let user_stack_ptr =
        unsafe { core::ptr::addr_of_mut!(USER_STACK).add(USER_STACK_SIZE) as *mut u8 };
    return user_stack_ptr;
}

/*
* 用于调用汇编函数，的宏
*/
macro_rules! call_asm_func {
    // 无参数时报错
    () => {
        compile_error!("Expected a function name");
    };
    // 匹配函数名
    ($func:ident) => {
        unsafe {
            asm!("call {}", sym $func);
        }
    };
}

#[panic_handler]
fn panic(_info : &PanicInfo) -> ! {
    print!("Kernel Panic: {}\n\r", _info);
    if let Some(location) = _info.location() {
        print!("Panic at {}:{} {}\n\r",
               location.file(),
               location.line(),
               _info.message());
    } else {
        print!("Panic: {}\n\r", _info.message());
    }
    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn rust_main() -> ! {
    uart_init();
    prints("Hello, riscv!\n\r");
    print!("Kernel Base: {}\n\r", "riscv64");
    loop {}
}

// #[unsafe(no_mangle)]
// pub extern "C" fn trap_handler() {
//     let mut mcause : usize;
//     unsafe {
//         asm!("csrr {}, mcause", out(reg) mcause);
//     }
//     print!("Trap: {:#x}\n\r", mcause);
//     if mcause & 0x8000000000000000 != 0 {
//         print!("Machine mode exception\n\r");
//     } else {
//         print!("User mode exception\n\r");
//         if mcause & 0x7FFFFFFF == 9 {
//             print!("ECALL from user mode\n\r");
//             unsafe {
//                 asm!("csrr sp, sscratch");
//                 asm!("csrw sstatus, t0");
//                 asm!("csrw sepc, t1");
//                 asm!("sret");
//             }
//         }
//     }
// }

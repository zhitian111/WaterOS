#![no_std]
#![no_main]
use core::arch::asm;
use core::option;
use core::panic::PanicInfo;
use water_os::io::stdout::{prints, uart_init};
use water_os::print;
use water_os::println;

pub const KERNEL_BASE : usize = 0xFFFF_FFC0_8000_0000;
pub const USER_BASE : usize = 0xC000_0000;

pub const USER_STACK_SIZE : usize = 0x0000_1000;
static mut USER_STACK : [u8; USER_STACK_SIZE] = [0; USER_STACK_SIZE];

pub static mut KERNEL_STACK_TOP_PTR : usize = 0xFFFF_FFC0_8000_1000;

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
    println!("Hello, riscv!");
    println!("Kernel Base: riscv64");
    println!("Regist S-Mode interrupt handler !");
    println!("rust_main address: {:#x}",
             rust_main as usize);
    let mut s_mode_trap_handler_ptr : usize = S_mode_trap_handler as usize;
    s_mode_trap_handler_ptr = s_mode_trap_handler_ptr & (!1);
    println!("S-Mode trap handler address: {:#x}",
             s_mode_trap_handler_ptr);
    unsafe {
        asm!("csrw stvec, {}", in(reg) s_mode_trap_handler_ptr);
    }
    println!("S-Mode interrupt handler set !");
    println!("Entering user mode !");
    println!("User stack top address: {:#x}",
             get_user_stack_top_ptr() as usize);
    println!("User entry point address: {:#x}",
             show_logo as usize);
    entry_user_mode(show_logo as usize);
    println!("Failed to enter user mode !");
    loop {}
}

// #[unsafe(no_mangle)]
// pub extern "C" fn trap_handler() {
//     let mut scause : usize;
//     unsafe {
//         asm!("csrr {}, scause", out(reg) mcause);
//     }
//     print!("Trap: {:#x}\n\r", scause);
//     if scause & 0x8000000000000000 != 0 {
//         print!("Machine mode exception\n\r");
//     } else {
//         print!("User mode exception\n\r");
//         if scause & 0x7FFFFFFF == 9 {
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
//

#[unsafe(no_mangle)]
#[unsafe(link_section = ".text.trap_handler")]
pub extern "C" fn S_mode_trap_handler() -> ! {
    unsafe {
        asm!("csrrw sp, sscratch, sp");
    }
    let mut scause : usize;
    let mut stval : usize;
    let mut sepc : usize;
    let mut sstatus : usize;
    unsafe {
        asm!("csrr {}, scause", out(reg) scause);
        asm!("csrr {}, stval", out(reg) stval);
        asm!("csrr {}, sepc", out(reg) sepc);
        asm!("csrr {}, sstatus", out(reg) sstatus);
    }
    print!("Trap: scause={:#x}, stval={:#x}, sepc={:#x}, sstatus={:#x}\n\r",
           scause, stval, sepc, sstatus);
    // 检查异常类型
    // Environment call form U-mode
    if scause ^ 8 == 0 {
        print!("Environment call from U-mode\n\r");
        unsafe {
            asm!("csrr t0, sepc",
                 "addi t0, t0, 4",
                 "csrw sepc, t0",
                 "csrrw sp, sscratch, sp",
                 "sret")
        }
    }
    // Environment call from S-mode
    if scause ^ 9 == 0 {
        print!("Environment call from S-mode\n\r");
    }

    loop {}
}

#[unsafe(no_mangle)]
pub extern "C" fn entry_user_mode(entry_point : usize) {
    let user_stack_top_ptr = get_user_stack_top_ptr() as usize;
    let user_sp = user_stack_top_ptr as usize - KERNEL_BASE + USER_BASE;
    let user_entry_point = entry_point - KERNEL_BASE + USER_BASE;
    println!("User stack top address: {:#x}", user_sp);
    println!("User entry point address: {:#x}",
             user_entry_point);
    let mut sie : usize;
    unsafe {
        asm!("csrsi sie, 9"); // SSIE=1
    }
    unsafe {
        asm!("csrr {}, sie", out(reg) sie);
    }
    println!("sie: {:#x}", sie);
    // 读入内核栈顶指针
    unsafe {
        asm!("mv {}, sp", out(reg) KERNEL_STACK_TOP_PTR);
        let kernel_stack_top_ptr = KERNEL_STACK_TOP_PTR;
        println!("Kernel stack top address: {:#x}",
                 kernel_stack_top_ptr);
        asm!("csrw sscratch, sp"); // 保存内核栈顶指针到sscratch
    }
    unsafe {
        asm!("mv sp, {user_sp}", // 用户栈指针
             "mv t0, {user_entry_point}",
             "csrw sepc, t0",
             "csrr t0, sstatus",
             "andi t0, t0, ~(1<<8)", // 设置SPP=0
             "ori t0, t0, (1<<5)", // 设置SIE=1
             "ori t0, t0, (1<<9)", // 设置SPIE=1
             "csrw sstatus, t0",
             user_sp=in(reg) user_sp,
             user_entry_point=in(reg) user_entry_point,
             clobber_abi("C"),
             out("t0") _
        )
    }
    let mut sstatus : usize;
    unsafe {
        asm!("csrr {}, sstatus", out(reg) sstatus);
    }
    println!("Current sstatus: {:#x}", sstatus);
    unsafe {
        asm!("sret");
    }
}

pub fn show_logo() {
    // println!(
    //          r#"
    // ██╗    ██╗ █████╗ ████████╗███████╗██████╗      ██████╗ ███████╗
    // ██║    ██║██╔══██╗╚══██╔══╝██╔════╝██╔══██╗    ██╔═══██╗██╔════╝
    // ██║ █╗ ██║███████║   ██║   █████╗  ██████╔╝    ██║   ██║███████╗
    // ██║███╗██║██╔══██║   ██║   ██╔══╝  ██╔══██╗    ██║   ██║╚════██║
    // ╚███╔███╔╝██║  ██║   ██║   ███████╗██║  ██║    ╚██████╔╝███████║
    //  ╚══╝╚══╝ ╚═╝  ╚═╝   ╚═╝   ╚══════╝╚═╝  ╚═╝     ╚═════╝ ╚══════╝"#
    // );
    unsafe {
        asm!("ecall");
    }
}

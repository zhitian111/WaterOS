#![no_std] // 不使用标准库
#![no_main] // 不使用main函数
use core::arch::asm;
use core::arch::global_asm;
use water_os::io::stdout::{prints, uart_init}; // 导入water_os的prints、uart_init、print!
global_asm!(include_str!("../../arch/riscv/entry.asm")); // 导入汇编代码
global_asm!(include_str!("../../arch/riscv/trap.asm"));
use core::panic::PanicInfo; // 导入PanicInfo
use water_os::print; // 导入输出宏

pub const KERNEL_BASE : usize = 0xFFFF_FFC0_0000_0000; // 内核基址

// 定义用户栈空间
pub const USER_STACK_SIZE : usize = 0x0000_0040; // 用户栈大小
static mut USER_STACK : [u8; USER_STACK_SIZE] = [0; USER_STACK_SIZE]; // 用户栈

// 获取用户栈顶指针
pub fn get_user_stack_top_ptr() -> *mut u8 {
    let user_stack_ptr =
        unsafe { core::ptr::addr_of_mut!(USER_STACK).add(USER_STACK_SIZE) as *mut u8 };
    return user_stack_ptr;
}

// 切换到用户态
pub fn switch_to_user_mode(user_entry : usize) -> ! {
    let user_stack_ptr = get_user_stack_top_ptr();
    // 设置用户态栈顶指针
    unsafe {
        asm!("mv sp, {stack_ptr}", stack_ptr = in(reg) user_stack_ptr);
        // 设置SEPC
        asm!("csrw sepc, {}", in(reg) user_entry);
        // 配置sstatus（SPP=0表示U-Mode，SPIE=1允许中断）
        asm!("csrr t0, sstatus",
             "li t1, 0x100",     // SPP位掩码（第8位）
             "not t1, t1",       // 取反用于清除位
             "and t0, t0, t1",   // 清除SPP位
             "ori t0, t0, 0x20", // 设置SPIE（第5位）
             "csrw sstatus, t0");
        // 切换到用户态
        asm!("sret");
    }
    loop {}
}
#[panic_handler] // 定义Panic处理函数
fn panic(_info : &PanicInfo) -> ! {
    print!("Kernel Panic: {}\n\r", _info);
    // 通过串口输出错误信息
    if let Some(location) = _info.location() {
        print!("Panic at {}:{} {}\n\r",
               location.file(),
               location.line(),
               _info.message());
    } else {
        print!("Panic: {}\n\r", _info.message());
    }
    //
    // // 进入紧急停机状态
    // loop {
    //     unsafe { riscv::asm::wfi(); } // 等待中断（实际可能需硬件特定操作）
    // }
    loop {}
}

#[unsafe(no_mangle)] // 不要对函数名称进行重命名
pub extern "C" fn rust_main() -> ! {
    uart_init();
    prints("Hello, riscv!\n\r"); // 输出Hello, riscv!到屏幕
    #[cfg(target_arch = "riscv64")] // 输出内核基址
    print!("Kernel Base: {}\n\r", "riscv64");
    #[cfg(target_arch = "loongarch64")] // 输出内核基址
    print!("Kernel Base: {}\n\r", "loongarch64");

    #[cfg(target_arch = "riscv64")] // 设置陷阱向量表
    unsafe {
        // 设置stvec指向陷阱处理程序
        asm!("la t0, trap_handler
              csrw stvec, t0");
        // 初始化sscratch用于上下文切换
        asm!("csrw sscratch, sp");
    }
    loop {}
}

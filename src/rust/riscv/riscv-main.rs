#![no_std] // 不使用标准库
#![no_main] // 不使用main函数
use core::arch::global_asm;
use water_os::io::stdout::*;
use water_os::io::stdout::{prints, uart_init}; // 导入water_os的prints、uart_init、print!
global_asm!(include_str!("../../arch/riscv/entry.asm")); // 导入汇编代码
use core::panic::PanicInfo; // 导入PanicInfo
use water_os::print; // 导入输出宏
pub const KERNEL_BASE : usize = 0xFFFF_FFC0_0000_0000; // 内核基址
#[panic_handler] // 定义Panic处理函数
fn panic(_info : &PanicInfo) -> ! {
    loop {}
}
use core::fmt::Write; // 导入Write
use water_os::io::stdout::BufferWriter; // 导入BufferWriter
#[unsafe(no_mangle)] // 不要对函数名称进行重命名
pub extern "C" fn rust_main() -> ! {
    uart_init();
    prints("Hello, riscv!\n\r"); // 输出Hello, riscv!到屏幕
    #[cfg(target_arch = "riscv64")] // 输出内核基址
    print!("Kernel Base: {}\n\r", "riscv64");
    #[cfg(target_arch = "loongarch64")] // 输出内核基址
    print!("Kernel Base: {}\n\r", "loongarch64");
    loop {}
}

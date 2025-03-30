#![no_std] // 不使用标准库
#![no_main] // 不使用main函数
use core::arch::global_asm;
use water_os::io::stdout::prints;
use water_os::io::stdout::uart_init; // 导入water_os的uart // 导入water_os的stdout // 导入汇编指令
global_asm!(include_str!("../../arch/riscv/entry.asm")); // 导入汇编代码
use core::panic::PanicInfo; // 导入PanicInfo

pub const KERNEL_BASE : usize = 0xFFFF_FFC0_0000_0000; // 内核基址
#[panic_handler] // 定义Panic处理函数
fn panic(_info : &PanicInfo) -> ! {
    loop {}
}

#[unsafe(no_mangle)] // 不要对函数名称进行重命名
pub extern "C" fn rust_main() -> ! {
    uart_init();
    prints("Hello, riscv!\n\r"); // 输出Hello, riscv!到屏幕
    loop {}
}

#![no_std] // 不使用标准库
#![no_main] // 不使用main函数

use core::arch::global_asm; // 导入汇编指令
                            // global_asm!(include_str!("./entry.asm")); // 导入汇编代码
use core::panic::PanicInfo; // 导入PanicInfo

#[panic_handler] // 定义Panic处理函数
fn panic(_info : &PanicInfo) -> ! {
    loop {}
}

#[unsafe(no_mangle)] // 不要对函数名称进行重命名
pub extern "C" fn rust_main() -> ! {
    loop {}
}

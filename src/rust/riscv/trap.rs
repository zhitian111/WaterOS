#![no_std]
#![no_main]
use core::panic::PanicInfo;
use water_os::kernal_log;
use water_os::print;

use core::arch::asm;

/**
# 方法简介
## 方法名称
panic
## 功能描述
打印错误信息，退出程序
## 处理流程
1. 打印错误信息；
2. 退出程序。
## 涉及数据
无
## 链式调用
water_os::kernel_log!()
water_os::print!()
# 输入参数
| 参数名 | 类型 | 含义 | 约束条件 | 默认值 |
| ------ | -------- | ------ | ------ | ------ |
| _info | &PanicInfo | 运行时错误信息 | 无 | 无 |
# 输出参数
| 参数名 | 类型 | 含义 | 约束条件 |
| ------ | -------- | ------ | ------ |

# 异常情况
| 异常类型 | 异常原因 | 异常处理方式 |
| ------ | -------- | ------ |
| any | unknown | 输出异常信息，退出程序 |
# 注意事项
如果你看到他被rust_analyzer标记为错误，请忽略
*/
#[panic_handler]
fn panic(_info : &PanicInfo) -> ! {
    kernal_log!("Kernel Panic: {}\n\r", _info);
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

/**
# 方法简介
## 方法名称
S_mode_trap_handler
## 功能描述
处理S态异常
## 处理流程
1. 打印异常信息；
2. 判断异常类型；
3. 处理异常；
## 涉及数据
无
## 链式调用
water_os::print!()
## 前置依赖
无
## 是否修改参数
会修改特权寄存器的值，包括：
- scause
- stval
- sepc
- sstatus
- sscratch
# 输入参数
| 参数名 | 类型 | 含义 | 约束条件 | 默认值 |
| ------ | -------- | ------ | ------ | ------ |
| 无 | 无 | 无 | 无 | 无 |
# 输出参数
| 参数名 | 类型 | 含义 | 约束条件 |
| ------ | -------- | ------ | ------ |
| 无 | 无 | 无 | 无 |
# 异常情况
| 异常类型 | 异常原因 | 异常处理方式 |
| ------ | -------- | ------ |
| Panic | 运行时错误 | 打印错误信息，退出程序 |
# 注意事项
这个函数一定要4K对齐，否则无法被正确调用。
*/
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

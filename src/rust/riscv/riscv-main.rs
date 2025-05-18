#![no_std]
#![no_main]
use core::arch::asm;
use water_os::def::*;
use water_os::fs::ext4::*;
use water_os::io::stdout::*;
use water_os::kernal_log;
use water_os::println;
mod trap;
mod virtual_devices;
mod virtual_memory;
pub const USER_BASE : usize = 0xC000_0000;

pub const USER_STACK_SIZE : usize = 0x0000_1000;
static mut USER_STACK : [u8; USER_STACK_SIZE] = [0; USER_STACK_SIZE];

pub static mut KERNEL_STACK_TOP_PTR : usize = 0xFFFF_FFC0_8000_1000;

pub fn get_user_stack_top_ptr() -> *mut u8 {
    let user_stack_ptr =
        unsafe { core::ptr::addr_of_mut!(USER_STACK).add(USER_STACK_SIZE) as *mut u8 };
    return user_stack_ptr;
}

/**
# 方法简介
## 方法名称
rust_main
## 功能描述
rust_main方法是内核由rust代码接管后的入口，目前包含以下流程：
- 初始化虚拟设备
- 显示logo
- 打印内核日志
- 设置S模式中断处理函数
- 测试用户态和内核态切换
## 涉及数据
如果展开调用来描述的话，它会涉及所有数据。
## 链式调用
如果展开调用来描述的话，它会涉及所有方法。
## 前置依赖
在项目下src/asm/riscv/wateros_platform_riscv64_gcc.S文件中，定义了一些汇编指令，其中的_start符号可被认为是rust_main函数的依赖。
## 是否修改参数
是
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
- 无
*/
#[unsafe(no_mangle)]
pub extern "C" fn rust_main() -> ! {
    virtual_devices::init_virtual_devices();
    show_logo();
    kernal_log!("Hello, riscv!");
    kernal_log!("Kernel Base: riscv64");
    kernal_log!("Regist S-Mode interrupt handler !");
    kernal_log!("rust_main address: {:#x}",
                rust_main as usize);
    let mut s_mode_trap_handler_ptr : usize = trap::S_mode_trap_handler as usize;
    s_mode_trap_handler_ptr = s_mode_trap_handler_ptr & (!1);
    kernal_log!("S-Mode trap handler address: {:#x}",
                s_mode_trap_handler_ptr);
    unsafe {
        asm!("csrw stvec, {}", in(reg) s_mode_trap_handler_ptr);
    }
    kernal_log!("S-Mode interrupt handler set !");
    kernal_log!("Entering user mode !");
    kernal_log!("User stack top address: {:#x}",
                get_user_stack_top_ptr() as usize);
    kernal_log!("User entry point address: {:#x}",
                show_logo as usize);
    kernal_log!("ext4_superblock size : {}",
                core::mem::size_of::<Ext4SuperBlock>());
    kernal_log!("ext4_group_block size : {}",
                core::mem::size_of::<Ext4BlockGroupDescriptor>());
    virtual_devices::print_dtb_info();
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
             // out("t0") _
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

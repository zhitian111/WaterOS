#![no_std] // 不使用标准库
#![no_main] // 不使用main函数

use core::arch::global_asm; // 导入汇编指令
use core::panic::PanicInfo; // 导入PanicInfo

// 导入我们将要创建的入口汇编文件
global_asm!(include_str!("../../asm/loongarch/entry.asm"));
global_asm!(include_str!("../../asm/loongarch/trap.asm"));

// 简单的打印函数，用于调试
fn print(s : &str) {
    // 在实际硬件上，这里应该通过串口或其他方式输出
    // 由于在Mac上无法直接运行，这里只提供一个框架
    #[cfg(target_arch = "loongarch64")]
    unsafe {
        // 模拟串口输出，通过内联汇编实现
        for byte in s.bytes() {
            // 假设0x10000000是串口数据寄存器的地址
            core::arch::asm!(
                "st.b {0}, {1}, 0",
                in(reg) byte,
                in(reg) 0x10000000u64
            );
        }
    }
}

// 用户程序函数
#[unsafe(no_mangle)]
pub extern "C" fn user_function() {
    print("在用户态执行函数！\n");

    // 调用系统调用
    unsafe {
        // 系统调用号为SYS_WRITE (2)
        core::arch::asm!("li.d $a7, 2", // 系统调用号放在a7寄存器
                         "li.d $a0, 1", // 文件描述符 stdout
                         "li.d $a1, 0", // 缓冲区地址（此处简化处理）
                         "li.d $a2, 0", // 长度（此处简化处理）
                         "syscall 0",   // 触发系统调用
                         options(noreturn));
    }
}

// 定义LoongArch寄存器结构体，用于保存上下文
#[repr(C)]
pub struct TrapFrame {
    // 通用寄存器 r0-r31
    pub regs : [usize; 32],
    // CSR寄存器
    pub csr_era : usize,   // 异常返回地址
    pub csr_prmd : usize,  // 先前运行模式信息
    pub csr_badv : usize,  // 出错的虚拟地址
    pub csr_ecfg : usize,  // 异常配置
    pub csr_estat : usize, // 异常状态
}

impl TrapFrame {
    pub fn new() -> Self {
        Self { regs : [0; 32],
               csr_era : 0,
               csr_prmd : 0,
               csr_badv : 0,
               csr_ecfg : 0,
               csr_estat : 0 }
    }
}

// 定义系统调用号
pub const SYS_EXIT : usize = 1;
pub const SYS_WRITE : usize = 2;
// 可以添加更多系统调用号

#[panic_handler] // 定义Panic处理函数
fn panic(info : &PanicInfo) -> ! {
    // 在实际实现中，这里应该输出一些调试信息
    loop {}
}

// 处理trap的函数，由trap.asm调用
#[unsafe(no_mangle)]
pub extern "C" fn trap_handler(tf : &mut TrapFrame) {
    // 获取trap类型
    let cause = tf.csr_estat & 0x7C; // 提取异常码字段

    match cause {
        0x0 => handle_interrupt(tf), // 中断
        0x4 => handle_tlb_error(tf), // TLB错误
        0xC => handle_syscall(tf),   // 系统调用
        _ => {
            // 其他异常处理
            // 在实际实现中应该打印详细信息并可能终止进程
            loop {}
        }
    }
}

// 处理中断
fn handle_interrupt(tf : &mut TrapFrame) {
    // 实现中断处理逻辑
}

// 处理TLB错误
fn handle_tlb_error(tf : &mut TrapFrame) {
    // 实现TLB错误处理逻辑
}

// 处理系统调用
fn handle_syscall(tf : &mut TrapFrame) {
    // 系统调用号在a7寄存器(r11)中，之前定义在a0中，这里修正
    let syscall_id = tf.regs[11]; // a7 寄存器

    print("回到内核态，系统调用号：");
    // 简单地打印系统调用号（在实际硬件上会显示）
    let syscall_str = match syscall_id {
        1 => "SYS_EXIT",
        2 => "SYS_WRITE",
        _ => "未知",
    };
    print(syscall_str);
    print("\n");

    // 参数在a0-a6寄存器(r4-r10)中
    let args = [tf.regs[4],
                tf.regs[5],
                tf.regs[6],
                tf.regs[7],
                tf.regs[8],
                tf.regs[9],
                tf.regs[10]];

    // 系统调用处理
    let result = match syscall_id {
        SYS_EXIT => sys_exit(args[0]),
        SYS_WRITE => sys_write(args[0], args[1] as *const u8, args[2]),
        _ => {
            // 未知系统调用
            -1isize as usize
        }
    };

    // 返回值存放在a0寄存器(r4)中
    tf.regs[4] = result;

    // 系统调用返回后，修改EPC寄存器，使其指向下一条指令
    tf.csr_era += 4;
}

// 系统调用：退出
fn sys_exit(exit_code : usize) -> usize {
    // 实际实现应该终止当前进程，返回给父进程
    // 这里简化处理，直接循环
    loop {}
}

// 系统调用：写入
fn sys_write(fd : usize, buffer : *const u8, size : usize) -> usize {
    // 简单实现，如果是标准输出，直接输出字符
    if fd == 1 {
        let slice = unsafe { core::slice::from_raw_parts(buffer, size) };
        // 实际实现应该将数据输出到控制台
        // 这里简化处理，什么也不做
        size
    } else {
        // 不支持的文件描述符
        usize::MAX
    }
}

// 切换到用户态
#[unsafe(no_mangle)]
pub fn switch_to_user(entry : usize, sp : usize) -> ! {
    print("即将进入用户态...\n");

    let mut tf = TrapFrame::new();

    // 设置用户程序入口点
    tf.csr_era = entry;

    // 设置用户栈
    tf.regs[3] = sp; // r3是sp寄存器

    // 设置PRMD以启用用户模式和中断
    tf.csr_prmd = 0x0; // 用户模式、开启异常

    // 跳转到汇编代码恢复上下文并返回用户态
    unsafe {
        restore_user_context(&tf);
    }

    // 这里不会执行到
    loop {}
}

// 外部汇编函数，恢复用户上下文并返回用户态

unsafe extern "C" {
    #[unsafe(no_mangle)]
    fn restore_user_context(tf : &TrapFrame) -> !;
}

#[unsafe(no_mangle)] // 不要对函数名称进行重命名
pub extern "C" fn rust_main() -> ! {
    print("WaterOS 内核启动...\n");

    // 设置异常向量表
    setup_trap_vector();

    print("初始化完成，准备切换到用户态...\n");

    // 用户栈和入口点
    const USER_STACK_SIZE : usize = 4096 * 2;
    static mut USER_STACK : [u8; USER_STACK_SIZE] = [0; USER_STACK_SIZE];

    // 切换到用户态程序
    let user_entry = user_function as usize;
    let user_stack = unsafe { core::ptr::addr_of_mut!(USER_STACK).add(USER_STACK_SIZE) as usize };

    switch_to_user(user_entry, user_stack);

    // 如果没有切换到用户态，就在内核模式下循环
    loop {}
}

// 设置异常向量表
fn setup_trap_vector() {
    unsafe extern "C" {
        fn trap_vector();
    }

    let vector_addr = trap_vector as usize;

    unsafe {
        // 设置CSR_EBASE（异常入口基址）
        core::arch::asm!("csrwr {}, 0xc", in(reg) vector_addr);

        // 开启中断
        // 设置ECFG寄存器，使能所需中断
        core::arch::asm!("csrwr {}, 0x4", in(reg) 0x1); // 使能时钟中断

        // 设置全局中断使能
        core::arch::asm!("csrwr {}, 0x0", in(reg) 0);
    }
}

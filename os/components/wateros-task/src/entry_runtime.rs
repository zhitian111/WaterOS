//! 与平台任务入口路径对接的运行时胶水：提供 **C ABI 符号**，供 arch 入口跳板
//! 在任务首次被调度时转入任务系统。
//!
//! 普通 trap/syscall/interrupt 的返回路径不经过本模块，而是由组合层
//! `os/src/trap_handler.rs` 调用 task crate 根上的 trap-frame 访问接口。

use crate::active_impl::TaskBootstrap;
use crate::scheduler;
use crate::trap_runtime;
use arch::trap::TrapAddressSpaceWrite;
// use riscv::register::sstatus;

use scheduler::TaskTrapFrame;

unsafe extern "C" {
    /// 平台 arch：按 trap 帧与内核栈顶恢复用户态执行；trap 帧中必须已写入返回地址空间 token。
    fn __wateros_arch_restore_user_task(trap_frame_ptr : *const u8, kernel_stack_top : usize) -> !;
}

/// 用户任务首次被调度后的入口路径：恢复 trap 帧、写入用户地址空间 token 并跳到 arch 恢复例程。
#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_enter_current_user_task() -> ! {
    let mut trap_frame = TaskTrapFrame::default();
    let restored = scheduler::restore_current_trap_frame(&mut trap_frame);
    let kernel_stack_top = scheduler::current_task_kernel_stack_top().expect("user task entry \
                                                                              requires a current \
                                                                              task kernel stack");
    assert!(restored,
            "user task entry requires a prepared trap frame in the current task");
    trap_frame.set_return_address_space_token(
        trap_runtime::current_user_return_address_space_token(),
    );
    unsafe {
        __wateros_arch_restore_user_task((&trap_frame as *const TaskTrapFrame).cast::<u8>(),
                                         kernel_stack_top)
    }
}

/// Idle 任务体：在内核态 `wfi` 等待中断。
///
/// **须**在首次进入时打开全局中断：[`schedule_tick`] 等路径在持有 [`InterruptGuard`] 时可能
/// `__switch` 到本任务，此时上一任务的 guard 尚未 `drop`，`SIE` 仍为关；若此处不 `enable`，
/// `wfi` 在 QEMU/常见 RISC-V 上可能等不到已挂起的 S 态定时器，表现为整机「卡死」在用户 `sret` 之后。
#[unsafe(no_mangle)]
pub extern "C" fn __wateros_idle_task_runtime_main(_arg : usize) -> ! {
    let _ = arch::interrupt::enable_global_interrupt();
    loop {
        arch::interrupt::wait_for_interrupt();
    }
}

/// 普通内核任务 arch 入口：`bootstrap_ptr` 指向 [`TaskBootstrap`]，启用中断后跳转到真实入口。
#[unsafe(no_mangle)]
pub extern "C" fn __wateros_task_runtime_entry(bootstrap_ptr : usize) -> ! {
    let bootstrap = unsafe { &*(bootstrap_ptr as *const TaskBootstrap) };
    arch::interrupt::enable_global_interrupt().expect("enable global interrupt for task runtime");
    bootstrap.run()
}

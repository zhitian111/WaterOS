//! **组合层内核 trap 路由**：实现原先 `impl-riscv64/trap.rs` 中的 `TrapCause`
//! 分支与返回路径，
//! 经 [`arch_api_v0::kernel_trap::register_kernel_trap_handler`] 挂接到
//! `trap_entry_rust`。
//!
//! **须在** `task::init()` **之后**调用 [`init`]。

use abi::user_ret::UserRet;
use arch_api_v0::kernel_trap::register_kernel_trap_handler;
use arch_api_v0::trap::{
    Exception, Interrupt, TrapCause, TrapFrameRead, TrapFrameWrite, TrapSyscallRead,
    TrapSyscallWrite,
};
use core::sync::atomic::{AtomicUsize, Ordering};
use platform::arch::paging;
use platform::arch::time::{read_time_tick, ArchTimeTick};
use platform::arch::trap::ActiveTrapFrame as TrapContext;
use platform::timer::set_timer_deadline_tick;
use riscv::register::sstatus;
use runtime::logging::*;
use syscall::dispatch_syscall_from_trap;
use task::trap_runtime;

/// 单次定时器中断后重新武装的切片长度（`time` CSR 刻度）；与调度策略相关，非用户 ABI。
const TIMER_SLICE_TICKS : u64 = 1_250_000;
static TIMER_TICK_COUNT : AtomicUsize = AtomicUsize::new(0);
/// RISC-V 上非压缩 `ecall` 占位长度，用于将 `sepc` 前进到返回到用户态的下一条指令。
const SYSCALL_INSN_BYTES : usize = 4;

/// 组合层内核 trap 入口：由 `arch` 在异常/中断向量中调用，`frame` 为当前 trap 帧字节指针。
///
/// 处理 `UserEnvCall`（分派 syscall）、页错日志、监督态定时器 tick（重武装 + 调度），其余
/// cause 直接 `panic`。若返回到用户态，在帧访问期间短暂开启 `SUM` 以便访问用户页。
extern "C" fn wateros_kernel_trap_handler(frame : *mut u8) {
    let authoritative = unsafe { trap_runtime::begin_current_trap_frame_access(frame) };
    let cx = unsafe { &mut *(authoritative as *mut TrapContext) };

    if cx.returns_to_user() {
        unsafe {
            sstatus::set_sum();
        }
    }
    let raw_scause = cx.raw_cause();
    let trap_cause = cx.trap_cause();
    match trap_cause {
        TrapCause::Exception(Exception::UserEnvCall) => {
            let syscall_nr = cx.syscall_nr()
                               .raw();
            let syscall_args = cx.syscall_args();
            let syscall_ret = dispatch_syscall_from_trap(syscall_nr, syscall_args);
            cx.add_user_pc(SYSCALL_INSN_BYTES);
            cx.set_syscall_ret(UserRet(syscall_ret));
        }
        TrapCause::Exception(Exception::InstructionPageFault) |
        TrapCause::Exception(Exception::LoadPageFault) |
        TrapCause::Exception(Exception::StorePageFault) => {
            debug!("[trap] page fault: cause={:?} scause={:#x?} sepc={:#x?} stval={:#x?}",
                   trap_cause,
                   raw_scause,
                   cx.user_pc(),
                   cx.fault_addr());
        }
        TrapCause::Interrupt(Interrupt::SupervisiorTimer) => {
            let now = read_time_tick().expect("read time tick during trap")
                                      .0;
            let deadline = now.saturating_add(TIMER_SLICE_TICKS);
            if let Err(err) = set_timer_deadline_tick(ArchTimeTick(deadline)) {
                panic!("failed to re-arm timer in trap: {:?}",
                       err);
            }
            let tick = TIMER_TICK_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
            if tick % 8 == 0 {
                trace!("[trap] timer tick {}", tick);
            }
            trap_runtime::schedule_tick_from_trap();
        }
        _ => {
            panic!("unexpected trap: cause={:?}, sepc={:#x}, stval={:#x}",
                   trap_cause,
                   cx.user_pc(),
                   cx.fault_addr());
        }
    }

    if cx.returns_to_user() {
        let satp = paging::read_satp();
        trace!("[trap] return to user sepc={:#x} x2/sp={:#x} satp={:#x} raw_scause={:#x}",
               cx.user_pc(),
               cx.user_sp(),
               satp,
               raw_scause);
    }

    trap_runtime::install_satp_for_exception_return(cx.returns_to_user());
    unsafe {
        let _ = trap_runtime::restore_current_trap_frame(frame);
    }
}

/// 向 `arch_api_v0` 注册本模块的 C ABI trap 回调；须在 `task::init()` 之后调用。
#[inline]
pub fn init() { register_kernel_trap_handler(wateros_kernel_trap_handler); }

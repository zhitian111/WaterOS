//! LoongArch64 **异常入口与帧语义**：与 `asm/trap.S` 中 `__alltraps` 保存顺序一致；
//! `TrapContext` 字段与 CSR 槽位（`prmd`/`era`/`estat`/`badv`）变更时必须同步汇编。
//!
//! **ABI**：用户态系统调用按 LoongArch Linux 约定使用 `$r4`–`r9` 为参数、`$r11` 为
//! 系统调用号；返回值写入 `$r4`（见 `TrapSyscallWrite`）。

use abi::syscall_args::SyscallArgs;
use abi::syscall_number::SyscallNumber;
use abi::user_ret::UserRet;
use api_v0::time::ArchTime;
use api_v0::trap::{
    Exception, Interrupt, TrapCause, TrapFrameRead, TrapFrameWrite, TrapSyscallRead,
    TrapSyscallWrite,
};
use core::arch::asm;
use core::sync::atomic::{AtomicUsize, Ordering};
use firmware::timer::FirmwareTimerDeadline;

unsafe extern "C" {
    fn __wateros_task_runtime_begin_current_trap_frame_access(trap_frame_ptr : *mut u8) -> *mut u8;
    fn __wateros_task_runtime_restore_current_trap_frame(trap_frame_ptr : *mut u8) -> bool;
    fn __wateros_syscall_dispatch_current(syscall_nr : usize,
                                          arg0 : usize,
                                          arg1 : usize,
                                          arg2 : usize,
                                          arg3 : usize,
                                          arg4 : usize,
                                          arg5 : usize)
                                          -> isize;
    fn __wateros_task_runtime_schedule_tick();
}

/// 字段顺序/大小必须与 `asm/trap.S` 的偏移保持一致。
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrapContext {
    x : [usize; 32],
    prmd : usize,
    era : usize,
    estat : usize,
    badv : usize,
}

/// 异常入口向量 CSR（`EENTRY`）：`trap.S` 中 `__alltraps` 的物理入口地址写入此 CSR。
const CSR_EENTRY : usize = 0xC;
/// 定时器中断清除 CSR：写 `TICLR` 指定位以清挂起（与 `firmware_set_timer`/硬件一致）。
const CSR_TICLR : usize = 0x44;
/// `PRMD.PPLV`：返回后特权级域（与 `returns_to_user` 判定一致）。
const LOONGARCH_PRMD_PPLV_MASK : usize = 0x3;
/// `PRMD.PIE`：返回时全局中断使能快照位（与 `set_return_to_user_raw` 配合）。
const LOONGARCH_PRMD_PIE : usize = 1 << 2;
/// 用户态 PLV 编码（与手册中 PLV=3 对应；用于区分返回到用户还是内核）。
const LOONGARCH_USER_PLV : usize = 0x3;
/// `ESTAT.IS.TI`：定时器中断挂起位（与 `decode_loongarch64_trap_cause` 一致）。
const TIMER_INTERRUPT_PENDING : usize = 1 << 11;
/// `TICLR.TI`：清除定时器中断挂起。
const TICLR_CLEAR_TIMER : usize = 1 << 0;
/// 每次时钟 tick 后重载固件 deadline 的 tick 增量（与 StableCounter 刻度一致；调参时改此处）。
const TIMER_SLICE_TICKS : u64 = 10_000_000;
/// 用户态 syscall 指令宽度（`era` 前进量）；与 trap 帧中 `era` 语义一致。
const SYSCALL_INSN_BYTES : usize = 4;
// 仅统计用；与调度 tick 路径耦合，便于将来调试或移除。
static TIMER_TICK_COUNT : AtomicUsize = AtomicUsize::new(0);

#[inline]
fn decode_loongarch64_trap_cause(estat : usize) -> TrapCause {
    if (estat & TIMER_INTERRUPT_PENDING) != 0 {
        return TrapCause::Interrupt(Interrupt::SupervisiorTimer);
    }

    let ecode = (estat >> 16) & 0x3F;
    match ecode {
        1 | 2 | 7 => TrapCause::Exception(Exception::LoadPageFault),
        3 | 6 => TrapCause::Exception(Exception::InstructionPageFault),
        4 => TrapCause::Exception(Exception::StorePageFault),
        9 => TrapCause::Exception(Exception::Breakpoint),
        11 => TrapCause::Exception(Exception::UserEnvCall),
        12 => TrapCause::Exception(Exception::Breakpoint),
        13 => TrapCause::Exception(Exception::IllegalInstruction),
        other => TrapCause::Exception(Exception::Unsupported(other)),
    }
}

impl TrapContext {
    #[inline]
    fn syscall_nr_raw(&self) -> usize {
        // LoongArch64 Linux ABI: a7($r11) 保存 syscall id。
        self.x[11]
    }

    #[inline]
    fn syscall_args_raw(&self) -> SyscallArgs {
        // LoongArch64 Linux ABI: a0..a5 依次是 $r4..$r9。
        SyscallArgs::from_regs([self.x[4], self.x[5], self.x[6], self.x[7], self.x[8], self.x[9]])
    }

    #[inline]
    fn user_sp_raw(&self) -> usize { self.x[3] }

    #[inline]
    fn set_user_sp_raw(&mut self, sp : usize) { self.x[3] = sp; }

    #[inline]
    fn returns_to_user_raw(&self) -> bool {
        (self.prmd & LOONGARCH_PRMD_PPLV_MASK) == LOONGARCH_USER_PLV
    }

    #[inline]
    fn set_return_to_user_raw(&mut self) {
        self.prmd = (self.prmd & !(LOONGARCH_PRMD_PPLV_MASK | LOONGARCH_PRMD_PIE)) |
                    LOONGARCH_USER_PLV |
                    LOONGARCH_PRMD_PIE;
    }

    #[inline]
    fn set_return_to_kernel_raw(&mut self) { self.prmd &= !LOONGARCH_PRMD_PPLV_MASK; }
}

unsafe extern "C" {
    fn __alltraps();
}

/// `csrwr`：写 CSR 并返回旧值；此处丢弃旧值，仅作副作用写。
#[inline]
fn write_csr<const CSR: usize>(value : usize) {
    let old = value;
    unsafe {
        asm!("csrwr {0}, {1}", inout(reg) old => _, const CSR);
    }
}

/// 安装异常入口：将 `__alltraps` 写入 `EENTRY`（与 `trap.S` 中符号地址一致）。
pub fn init_trap() {
    let addr = __alltraps as *const () as usize;
    write_csr::<CSR_EENTRY>(addr);
}

/// 汇编 `bl trap_entry_rust` 转入：在权威 trap 帧指针上完成 syscall / 定时器 /
/// 其它异常的最小分发；返回前经运行时恢复帧（与 RISC-V 的 `kernel_trap` 单入口模式不同）。
#[unsafe(no_mangle)]
pub extern "C" fn trap_entry_rust(cx_ptr : *mut TrapContext) {
    let authoritative_cx_ptr =
        unsafe { __wateros_task_runtime_begin_current_trap_frame_access(cx_ptr.cast::<u8>()) };
    let cx = unsafe { &mut *(authoritative_cx_ptr as *mut TrapContext) };

    match cx.trap_cause() {
        TrapCause::Exception(Exception::UserEnvCall) => {
            handle_user_syscall(cx);
        }
        TrapCause::Exception(Exception::InstructionPageFault) |
        TrapCause::Exception(Exception::LoadPageFault) |
        TrapCause::Exception(Exception::StorePageFault) => {
            let _ = (cx.estat, cx.era, cx.badv);
        }
        TrapCause::Interrupt(Interrupt::SupervisiorTimer) => {
            write_csr::<CSR_TICLR>(TICLR_CLEAR_TIMER);
            let now =
                super::time::LoongArch64ArchTime::read_time_tick().expect("read loongarch64 time \
                                                                           tick during trap")
                                                                  .0;
            let deadline = now.saturating_add(TIMER_SLICE_TICKS);
            if let Err(err) = firmware::timer::set_timer(FirmwareTimerDeadline(deadline)) {
                panic!("failed to re-arm loongarch64 timer in trap: {:?}",
                       err);
            }
            let _tick = TIMER_TICK_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
            unsafe {
                __wateros_task_runtime_schedule_tick();
            }
        }
        trap_cause => {
            panic!("unexpected loongarch64 trap: cause={:?}, era={:#x}, badv={:#x}, estat={:#x}",
                   trap_cause, cx.era, cx.badv, cx.estat);
        }
    }

    unsafe {
        __wateros_task_runtime_restore_current_trap_frame(cx_ptr.cast::<u8>());
    }
}

/// 用户态 `syscall`：经链接的运行时符号派发，结果写回 `UserRet` 对应寄存器槽。
fn handle_user_syscall(cx : &mut TrapContext) {
    let syscall_nr = cx.syscall_nr()
                       .raw();
    let syscall_args = cx.syscall_args();
    let syscall_ret = unsafe {
        __wateros_syscall_dispatch_current(syscall_nr,
                                           syscall_args.arg(0),
                                           syscall_args.arg(1),
                                           syscall_args.arg(2),
                                           syscall_args.arg(3),
                                           syscall_args.arg(4),
                                           syscall_args.arg(5))
    };
    cx.add_user_pc(SYSCALL_INSN_BYTES);
    cx.set_syscall_ret(UserRet(syscall_ret));
}

impl TrapFrameRead for TrapContext {
    fn raw_cause(&self) -> usize { self.estat }

    fn trap_cause(&self) -> TrapCause { decode_loongarch64_trap_cause(self.estat) }

    fn fault_addr(&self) -> usize { self.badv }

    fn user_pc(&self) -> usize { self.era }

    fn user_sp(&self) -> usize { self.user_sp_raw() }

    fn returns_to_user(&self) -> bool { self.returns_to_user_raw() }
}

impl TrapSyscallRead for TrapContext {
    fn syscall_args(&self) -> SyscallArgs { self.syscall_args_raw() }

    fn syscall_nr(&self) -> SyscallNumber { SyscallNumber(self.syscall_nr_raw()) }
}

impl TrapFrameWrite for TrapContext {
    fn set_user_pc(&mut self, pc : usize) { self.era = pc; }

    fn add_user_pc(&mut self, bytes : usize) {
        self.era = self.era
                       .wrapping_add(bytes);
    }

    fn set_user_sp(&mut self, sp : usize) { self.set_user_sp_raw(sp); }

    fn set_return_to_user(&mut self) { self.set_return_to_user_raw(); }

    fn set_return_to_kernel(&mut self) { self.set_return_to_kernel_raw(); }
}

impl TrapSyscallWrite for TrapContext {
    fn set_syscall_ret(&mut self, ret : UserRet) { self.x[4] = ret.0 as usize; }
}

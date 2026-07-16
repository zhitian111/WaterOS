//! RISC-V S 模式 **trap 帧与最小入口**：与 `asm/trap.asm` 中 `__alltraps` /
//! [`trap_entry_rust`] 对齐。
//!
//! **路由**：业务在组合层经 [`wateros_platform_arch_api_v0::kernel_trap`]
//! 注册；本文件只做帧类型与 **一次** `invoke_kernel_trap_handler`，不依赖
//! `task`/`syscall`。

use abi::syscall_args::SyscallArgs;
use abi::syscall_number::SyscallNumber;
use abi::user_ret::UserRet;
use api_v0::kernel_trap;
use api_v0::trap::{
    Exception, Interrupt, SignalFrameCodec, SignalMachineContext, TrapCause, TrapFrameRead,
    TrapFrameWrite,
};
use core::arch::asm;
use riscv::register::sstatus;

unsafe extern "C" {
    static mut __wateros_riscv_kernel_satp: usize;
}

/// 该结构的字段顺序/大小必须与 `asm/trap.asm` 的偏移严格一致（方案A）。
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrapContext {
    pub(crate) x : [usize; 32],
    pub(crate) sstatus : usize,
    pub(crate) sepc : usize,
    pub(crate) scause : usize,
    pub(crate) stval : usize,
    /// satp
    pub(crate) return_address_space_token : usize,
}

const RISCV_SSTATUS_SPP : usize = 1 << 8;
const RISCV_SSTATUS_FS_DIRTY : usize = 3 << 13;
/// 单次定时器中断后重新武装的切片长度（`time` CSR 刻度）；与调度策略相关，非用户 ABI。
const TIMER_SLICE_TICKS : u64 = 1_250_000;

/// 写入 trap trampoline 在用户页表下可读取的内核 `satp` 槽位。
///
/// 用户态 trap 入口会先用这枚 token 切回内核页表，然后再运行 Rust trap handler。
pub fn set_kernel_trap_satp(token : usize) {
    unsafe {
        core::ptr::write_volatile(core::ptr::addr_of_mut!(__wateros_riscv_kernel_satp),
                                  token);
    }
}

unsafe fn save_fp_state() -> ([u64; 32], u32) {
    let mut regs = [0u64; 32];
    let mut fcsr : usize;
    let base = regs.as_mut_ptr();
    unsafe {
        asm!(
            "fsd f0, 0({base})", "fsd f1, 8({base})",
            "fsd f2, 16({base})", "fsd f3, 24({base})",
            "fsd f4, 32({base})", "fsd f5, 40({base})",
            "fsd f6, 48({base})", "fsd f7, 56({base})",
            "fsd f8, 64({base})", "fsd f9, 72({base})",
            "fsd f10, 80({base})", "fsd f11, 88({base})",
            "fsd f12, 96({base})", "fsd f13, 104({base})",
            "fsd f14, 112({base})", "fsd f15, 120({base})",
            "fsd f16, 128({base})", "fsd f17, 136({base})",
            "fsd f18, 144({base})", "fsd f19, 152({base})",
            "fsd f20, 160({base})", "fsd f21, 168({base})",
            "fsd f22, 176({base})", "fsd f23, 184({base})",
            "fsd f24, 192({base})", "fsd f25, 200({base})",
            "fsd f26, 208({base})", "fsd f27, 216({base})",
            "fsd f28, 224({base})", "fsd f29, 232({base})",
            "fsd f30, 240({base})", "fsd f31, 248({base})",
            "csrr {fcsr}, fcsr",
            base = in(reg) base,
            fcsr = out(reg) fcsr,
            options(nostack),
        );
    }
    (regs, fcsr as u32)
}

unsafe fn restore_fp_state(regs : &[u64; 32], fcsr : u32) {
    let base = regs.as_ptr();
    unsafe {
        asm!(
            "fld f0, 0({base})", "fld f1, 8({base})",
            "fld f2, 16({base})", "fld f3, 24({base})",
            "fld f4, 32({base})", "fld f5, 40({base})",
            "fld f6, 48({base})", "fld f7, 56({base})",
            "fld f8, 64({base})", "fld f9, 72({base})",
            "fld f10, 80({base})", "fld f11, 88({base})",
            "fld f12, 96({base})", "fld f13, 104({base})",
            "fld f14, 112({base})", "fld f15, 120({base})",
            "fld f16, 128({base})", "fld f17, 136({base})",
            "fld f18, 144({base})", "fld f19, 152({base})",
            "fld f20, 160({base})", "fld f21, 168({base})",
            "fld f22, 176({base})", "fld f23, 184({base})",
            "fld f24, 192({base})", "fld f25, 200({base})",
            "fld f26, 208({base})", "fld f27, 216({base})",
            "fld f28, 224({base})", "fld f29, 232({base})",
            "fld f30, 240({base})", "fld f31, 248({base})",
            "csrw fcsr, {fcsr}",
            base = in(reg) base,
            fcsr = in(reg) fcsr as usize,
            options(nostack),
        );
    }
}

#[repr(transparent)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scause(pub usize);

/// RISC-V `scause`：最高位为 1 表示中断，否则为异常；低位为原因码（见 Privileged Spec）。
const RISCV_SCAUSE_INTERRUPT_BIT : usize = 1usize << (usize::BITS - 1);

impl From<Scause> for TrapCause {
    fn from(Scause(scause) : Scause) -> Self {
        if scause & RISCV_SCAUSE_INTERRUPT_BIT != 0 {
            let code = scause & !RISCV_SCAUSE_INTERRUPT_BIT;
            match code {
                1 => TrapCause::Interrupt(Interrupt::SupervisiorSoft),
                5 => TrapCause::Interrupt(Interrupt::SupervisiorTimer),
                9 => TrapCause::Interrupt(Interrupt::SupervisiorExternel),
                c => TrapCause::Interrupt(Interrupt::Unsupported(c)),
            }
        } else {
            match scause {
                2 => TrapCause::Exception(Exception::IllegalInstruction),
                3 => TrapCause::Exception(Exception::Breakpoint),
                8 => TrapCause::Exception(Exception::UserEnvCall),
                12 => TrapCause::Exception(Exception::InstructionPageFault),
                13 => TrapCause::Exception(Exception::LoadPageFault),
                15 => TrapCause::Exception(Exception::StorePageFault),
                c => TrapCause::Exception(Exception::Unsupported(c)),
            }
        }
    }
}


unsafe extern "C" {
    fn __alltraps();
}

/// 初始化 trap 入口：把 `stvec` 指向 `__alltraps`。
pub fn init_trap() {
    let addr = __alltraps as *const () as usize;
    let stvec = addr & !0x3;
    unsafe {
        asm!("csrw stvec, {0}", in(reg) stvec);
    }
}

/// RISC-V 返回用户态前允许内核在 trap 处理期间访问用户页。
#[inline]
pub fn prepare_user_trap_frame_access() {
    unsafe {
        sstatus::set_sum();
    }
}

/// 当前 RISC-V trap 调度时间片长度。
#[inline]
pub const fn timer_slice_ticks() -> u64 { TIMER_SLICE_TICKS }

/// 汇编入口转入：交给组合层 [`kernel_trap::invoke_kernel_trap_handler`]。
#[unsafe(no_mangle)]
pub extern "C" fn trap_entry_rust(cx_ptr : *mut TrapContext) {
    kernel_trap::invoke_kernel_trap_handler(cx_ptr.cast());
}

impl TrapFrameRead for TrapContext {
    fn raw_cause(&self) -> usize { self.scause }

    fn trap_cause(&self) -> TrapCause { TrapCause::from(Scause(self.scause)) }

    fn fault_addr(&self) -> usize { self.stval }

    fn user_pc(&self) -> usize { self.sepc }

    fn user_sp(&self) -> usize { self.x[2] }

    fn user_tls(&self) -> usize { self.x[4] }

    fn returns_to_user(&self) -> bool { (self.sstatus & RISCV_SSTATUS_SPP) == 0 }

    fn return_address_space_token(&self) -> usize { self.return_address_space_token }
    fn syscall_args(&self) -> SyscallArgs {
        SyscallArgs::from_regs([self.x[10], self.x[11], self.x[12], self.x[13], self.x[14],
                                self.x[15]])
    }

    fn syscall_nr(&self) -> SyscallNumber { SyscallNumber(self.x[17]) }
}


impl TrapFrameWrite for TrapContext {
    fn set_user_pc(&mut self, pc : usize) { self.sepc = pc; }

    fn add_user_pc(&mut self, bytes : usize) {
        self.sepc = self.sepc
                        .wrapping_add(bytes);
    }

    fn set_user_sp(&mut self, sp : usize) { self.x[2] = sp; }

    fn set_user_entry_args(&mut self, _argc : usize, _argv : usize, _envp : usize) {
        // Linux/RISC-V libc 从用户栈读 argc/argv/envp；a0 预留给 rtld_fini，静态链接须为 0。
        self.x[10] = 0;
    }

    fn set_return_to_user(&mut self) {
        self.sstatus &= !RISCV_SSTATUS_SPP;
        self.sstatus &= !(1 << 1);
        self.sstatus |= 1 << 5;
        // 用户态按 riscv64gc/lp64d 构建，libc 启动期可能执行 F/D 指令；完整 FPU
        // 上下文切换落地前，先保持 FS 位使能。
        self.sstatus |= RISCV_SSTATUS_FS_DIRTY;
    }

    fn set_return_to_kernel(&mut self) { self.sstatus |= RISCV_SSTATUS_SPP; }
    fn set_return_address_space_token(&mut self, token : usize) {
        self.return_address_space_token = token;
    }
    fn set_syscall_ret(&mut self, ret : UserRet) { self.x[10] = ret.0 as usize; }
    fn set_user_tls(&mut self, tls : usize) {
        // RISC-V psABI：线程指针为 x4（`tp`）。
        self.x[4] = tls;
    }
}


impl SignalFrameCodec for TrapContext {
    fn capture_signal_context(&self) -> SignalMachineContext {
        let (fpregs, fcsr) = unsafe { save_fp_state() };
        SignalMachineContext { gprs : self.x,
                               pc : self.sepc,
                               status : self.sstatus,
                               fpregs,
                               fcsr,
                               reserved : 0 }
    }

    fn restore_signal_context(&mut self, context : &SignalMachineContext) -> bool {
        if context.pc == 0 || context.pc & 1 != 0 {
            return false;
        }
        self.x = context.gprs;
        self.x[0] = 0;
        self.sepc = context.pc;
        unsafe {
            restore_fp_state(&context.fpregs, context.fcsr);
        }
        self.set_return_to_user();
        true
    }

    fn prepare_signal_handler(&mut self,
                              handler : usize,
                              restorer : usize,
                              frame_sp : usize,
                              signal : usize,
                              siginfo : usize,
                              ucontext : usize) {
        self.x[1] = restorer;
        self.x[2] = frame_sp;
        self.x[10] = signal;
        self.x[11] = siginfo;
        self.x[12] = ucontext;
        self.sepc = handler;
        self.set_return_to_user();
    }

    fn prepare_syscall_restart(context : &mut SignalMachineContext,
                               syscall_nr : usize,
                               args : [usize; 6],
                               instruction_bytes : usize) {
        context.pc = context.pc
                            .wrapping_sub(instruction_bytes);
        context.gprs[10..16].copy_from_slice(&args);
        context.gprs[17] = syscall_nr;
    }
}

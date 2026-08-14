//! RISC-V S 模式 **trap 帧与最小入口**：与 `asm/trap.asm` 中 `__alltraps` /
//! [`trap_entry_rust`] 对齐。
//!
//! **路由**：业务在组合层经 [`wateros_platform_arch_api_v0::kernel_trap`]
//! 注册；本文件只做帧类型与 **一次** `invoke_kernel_trap_handler`，不依赖
//! `task`/`syscall`。

use syscall_api::{SyscallArgs, SyscallNumber, UserRet};
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
    /// 用户态 `f0`–`f31`。trap 入口在调用 Rust 前保存，返回前恢复，避免
    /// 抢占式任务切换让不同用户线程串用硬件浮点寄存器。
    pub(crate) fpregs : [u64; 32],
    /// RISC-V 浮点控制/状态寄存器。
    pub(crate) fcsr : u32,
}

const _ : () = assert!(core::mem::offset_of!(TrapContext, fpregs) == 37 * 8);
const _ : () = assert!(core::mem::offset_of!(TrapContext, fcsr) == 69 * 8);
const _ : () = assert!(core::mem::size_of::<TrapContext>() == 70 * 8);

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

/// RISC-V 内核代码依赖内核 `satp` 中的映射，用户 trap 进入 Rust 前必须切换
/// 到内核地址空间。
#[inline]
pub const fn user_trap_requires_kernel_address_space() -> bool { true }

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
        // 用户态按 riscv64gc/lp64d 构建，libc 启动期可能执行 F/D 指令；trap
        // 入口/返回会完整保存和恢复 FPU 状态，因此始终保持 FS 位使能。
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
        SignalMachineContext { gprs : self.x,
                               pc : self.sepc,
                               status : self.sstatus,
                               fpregs : self.fpregs,
                               fcsr : self.fcsr,
                               fcc : 0,
                               vectors : [[0; 2]; 32] }
    }

    fn restore_signal_context(&mut self, context : &SignalMachineContext) -> bool {
        if context.pc == 0 || context.pc & 1 != 0 {
            return false;
        }
        self.x = context.gprs;
        self.x[0] = 0;
        self.sepc = context.pc;
        self.fpregs = context.fpregs;
        self.fcsr = context.fcsr;
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

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
    Exception, Interrupt, TrapCause, TrapFrameRead, TrapFrameWrite, TrapSyscallRead,
    TrapSyscallWrite,
};
use core::arch::asm;

/// 该结构的字段顺序/大小必须与 `asm/trap.asm` 的偏移严格一致（方案A）。
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrapContext {
    pub(crate) x : [usize; 32],
    pub(crate) sstatus : usize,
    pub(crate) sepc : usize,
    pub(crate) scause : usize,
    pub(crate) stval : usize,
}

const RISCV_SSTATUS_SPP : usize = 1 << 8;

/// RISC-V 监管态 **`scause` CSR** 原始值；仅在本 crate 内表达「数值来自 `scause`」，
/// 以便实现 **`From<Scause> for TrapCause`**（解码逻辑架构敏感，不属于 `arch-api`）。
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

impl TrapContext {
    #[inline]
    pub const fn raw_cause(&self) -> usize { self.scause }

    #[inline]
    pub const fn user_pc(&self) -> usize { self.sepc }

    #[inline]
    pub const fn user_sp(&self) -> usize { self.x[2] }

    #[inline]
    pub const fn fault_addr(&self) -> usize { self.stval }

    #[inline]
    pub const fn returns_to_user(&self) -> bool { (self.sstatus & RISCV_SSTATUS_SPP) == 0 }

    #[inline]
    pub const fn returns_to_kernel(&self) -> bool { !self.returns_to_user() }

    #[inline]
    // `a7` = x17：RISC-V syscall 调用号约定，与 `SyscallNumber` 对应。
    fn syscall_nr_raw(&self) -> usize { self.x[17] }

    #[inline]
    // `a0`–`a5` = x10–x15：与 `SyscallArgs::from_regs` 顺序一致。
    fn syscall_args_raw(&self) -> SyscallArgs {
        SyscallArgs::from_regs([self.x[10], self.x[11], self.x[12], self.x[13], self.x[14],
                                self.x[15]])
    }

    #[inline]
    fn user_sp_raw(&self) -> usize { self.x[2] }

    #[inline]
    fn set_user_sp_raw(&mut self, sp : usize) { self.x[2] = sp; }

    #[inline]
    fn returns_to_user_raw(&self) -> bool { (self.sstatus & RISCV_SSTATUS_SPP) == 0 }

    #[inline]
    // 清 `SPP`（返回到 U 态）、清 `SIE` 避免 `sret` 瞬间嵌套、置 `SPIE` 以便在用户态恢复中断。
    fn set_return_to_user_raw(&mut self) {
        self.sstatus &= !RISCV_SSTATUS_SPP;
        self.sstatus &= !(1 << 1);
        self.sstatus |= 1 << 5;
    }

    #[inline]
    fn set_return_to_kernel_raw(&mut self) { self.sstatus |= RISCV_SSTATUS_SPP; }
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

/// 汇编入口转入：交给组合层 [`kernel_trap::invoke_kernel_trap_handler`]。
#[unsafe(no_mangle)]
pub extern "C" fn trap_entry_rust(cx_ptr : *mut TrapContext) {
    kernel_trap::invoke_kernel_trap_handler(cx_ptr.cast());
}

impl TrapFrameRead for TrapContext {
    fn raw_cause(&self) -> usize {
        self.scause
    }

    fn trap_cause(&self) -> TrapCause {
        TrapCause::from(Scause(self.scause))
    }

    fn fault_addr(&self) -> usize {
        self.stval
    }

    fn user_pc(&self) -> usize { self.sepc }

    fn user_sp(&self) -> usize { self.user_sp_raw() }

    fn returns_to_user(&self) -> bool { self.returns_to_user_raw() }
}

impl TrapSyscallRead for TrapContext {
    fn syscall_args(&self) -> SyscallArgs { self.syscall_args_raw() }

    fn syscall_nr(&self) -> SyscallNumber { SyscallNumber(self.syscall_nr_raw()) }
}

impl TrapFrameWrite for TrapContext {
    fn set_user_pc(&mut self, pc : usize) { self.sepc = pc; }

    fn add_user_pc(&mut self, bytes : usize) {
        self.sepc = self.sepc
                        .wrapping_add(bytes);
    }

    fn set_user_sp(&mut self, sp : usize) { self.set_user_sp_raw(sp); }

    fn set_return_to_user(&mut self) { self.set_return_to_user_raw(); }

    fn set_return_to_kernel(&mut self) { self.set_return_to_kernel_raw(); }
}

impl TrapSyscallWrite for TrapContext {
    fn set_syscall_ret(&mut self, ret : UserRet) { self.x[10] = ret.0 as usize; }
}

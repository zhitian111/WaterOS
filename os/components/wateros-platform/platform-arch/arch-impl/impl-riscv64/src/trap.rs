//! RISC-V S 模式 **trap 帧与最小入口**：与 `asm/trap.asm` 中 `__alltraps` / [`trap_entry_rust`] 对齐。
//!
//! **路由**：业务在组合层经 [`wateros_platform_arch_api_v0::kernel_trap`] 注册；本文件只做帧类型与
//! **一次** `invoke_kernel_trap_handler`，不依赖 `task`/`syscall`。

use abi::syscall_args::SyscallArgs;
use abi::syscall_number::SyscallNumber;
use abi::user_ret::UserRet;
use api_v0::kernel_trap;
use api_v0::trap::{
    TrapFrameRead, TrapFrameWrite, TrapSyscallRead, TrapSyscallWrite,
};
use core::arch::asm;

/// 该结构的字段顺序/大小必须与 `asm/trap.asm` 的偏移严格一致（方案A）。
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TrapContext {
    pub(crate) x: [usize; 32],
    pub(crate) sstatus: usize,
    pub(crate) sepc: usize,
    pub(crate) scause: usize,
    pub(crate) stval: usize,
}

const RISCV_SSTATUS_SPP: usize = 1 << 8;

impl TrapContext {
    #[inline]
    pub const fn raw_cause(&self) -> usize {
        self.scause
    }

    #[inline]
    pub const fn user_pc(&self) -> usize {
        self.sepc
    }

    #[inline]
    pub const fn user_sp(&self) -> usize {
        self.x[2]
    }

    #[inline]
    pub const fn fault_addr(&self) -> usize {
        self.stval
    }

    #[inline]
    pub const fn returns_to_user(&self) -> bool {
        (self.sstatus & RISCV_SSTATUS_SPP) == 0
    }

    #[inline]
    pub const fn returns_to_kernel(&self) -> bool {
        !self.returns_to_user()
    }

    #[inline]
    fn syscall_nr_raw(&self) -> usize {
        self.x[17]
    }

    #[inline]
    fn syscall_args_raw(&self) -> SyscallArgs {
        SyscallArgs::from_regs([
            self.x[10],
            self.x[11],
            self.x[12],
            self.x[13],
            self.x[14],
            self.x[15],
        ])
    }

    #[inline]
    fn user_sp_raw(&self) -> usize {
        self.x[2]
    }

    #[inline]
    fn set_user_sp_raw(&mut self, sp: usize) {
        self.x[2] = sp;
    }

    #[inline]
    fn returns_to_user_raw(&self) -> bool {
        (self.sstatus & RISCV_SSTATUS_SPP) == 0
    }

    #[inline]
    fn set_return_to_user_raw(&mut self) {
        self.sstatus &= !RISCV_SSTATUS_SPP;
        self.sstatus &= !(1 << 1);
        self.sstatus |= 1 << 5;
    }

    #[inline]
    fn set_return_to_kernel_raw(&mut self) {
        self.sstatus |= RISCV_SSTATUS_SPP;
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

/// 汇编入口转入：交给组合层 [`kernel_trap::invoke_kernel_trap_handler`]。
#[unsafe(no_mangle)]
pub extern "C" fn trap_entry_rust(cx_ptr: *mut TrapContext) {
    kernel_trap::invoke_kernel_trap_handler(cx_ptr.cast());
}

impl TrapFrameRead for TrapContext {
    fn raw_cause(&self) -> usize {
        self.scause
    }

    fn fault_addr(&self) -> usize {
        self.stval
    }

    fn user_pc(&self) -> usize {
        self.sepc
    }

    fn user_sp(&self) -> usize {
        self.user_sp_raw()
    }

    fn returns_to_user(&self) -> bool {
        self.returns_to_user_raw()
    }
}

impl TrapSyscallRead for TrapContext {
    fn syscall_args(&self) -> SyscallArgs {
        self.syscall_args_raw()
    }

    fn syscall_nr(&self) -> SyscallNumber {
        SyscallNumber(self.syscall_nr_raw())
    }
}

impl TrapFrameWrite for TrapContext {
    fn set_user_pc(&mut self, pc: usize) {
        self.sepc = pc;
    }

    fn add_user_pc(&mut self, bytes: usize) {
        self.sepc = self.sepc.wrapping_add(bytes);
    }

    fn set_user_sp(&mut self, sp: usize) {
        self.set_user_sp_raw(sp);
    }

    fn set_return_to_user(&mut self) {
        self.set_return_to_user_raw();
    }

    fn set_return_to_kernel(&mut self) {
        self.set_return_to_kernel_raw();
    }
}

impl TrapSyscallWrite for TrapContext {
    fn set_syscall_ret(&mut self, ret: UserRet) {
        self.x[10] = ret.0 as usize;
    }
}

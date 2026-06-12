//! Trap 帧、异常与中断原因，以及与用户态系统调用 ABI 的读写桥接（**纯架构语义**）。
//!
//! ## `TrapCause` 与架构敏感解码
//!
//! - **`TrapCause` / `Exception` / `Interrupt`** 是跨架构的 **语义** 枚举；**原始 CSR 或硬件原因码
//!   如何映射到 `TrapCause` 由各 `arch-impl` 负责**（例如 RISC-V 在 `impl-riscv64` 用 `Scause`
//!   与 `From<Scause> for TrapCause`）。
//! - **`TrapFrameRead::trap_cause`** 必须由每个 trap 帧类型 **显式实现**；`arch-api` **不**提供
//!   从裸 `usize` 到 `TrapCause` 的默认转换，以免把某一 ISA 的编码误当成通用契约。

use abi::syscall_args::{SyscallArgs, SyscallPacket};
use abi::syscall_number::SyscallNumber;
use abi::user_ret::UserRet;
#[allow(unused)]
#[derive(Clone, Copy, Debug)]
pub enum Exception {
    UserEnvCall,
    InstructionPageFault,
    LoadPageFault,
    StorePageFault,
    IllegalInstruction,
    Breakpoint,
    Unsupported(usize),
}
#[allow(unused)]
#[derive(Clone, Copy, Debug)]
pub enum Interrupt {
    SupervisiorTimer,
    SupervisiorExternel,
    SupervisiorSoft,
    Unsupported(usize),
}
#[allow(unused)]
#[derive(Clone, Copy, Debug)]
pub enum TrapCause {
    Exception(Exception),
    Interrupt(Interrupt),
}

impl TrapCause {
    #[inline]
    #[allow(unused)]
    pub fn is_exception(&self) -> bool {
        matches!(self, TrapCause::Exception(_))
    }

    #[inline]
    #[allow(unused)]
    pub fn is_interrupt(&self) -> bool {
        matches!(self, TrapCause::Interrupt(_))
    }

    #[inline]
    #[allow(unused)]
    pub fn as_exception(self) -> Option<Exception> {
        match self {
            TrapCause::Exception(exception) => Some(exception),
            TrapCause::Interrupt(_) => None,
        }
    }

    #[inline]
    #[allow(unused)]
    pub fn as_interrupt(self) -> Option<Interrupt> {
        match self {
            TrapCause::Exception(_) => None,
            TrapCause::Interrupt(interrupt) => Some(interrupt),
        }
    }
}

#[allow(unused)]
pub trait TrapFrameRead {
    fn raw_cause(&self) -> usize;

    /// 将 `raw_cause()` 的 **架构相关** 原始编码解码为跨架构语义 `TrapCause`。
    fn trap_cause(&self) -> TrapCause;

    fn fault_addr(&self) -> usize;
    fn user_pc(&self) -> usize;
    fn user_sp(&self) -> usize;
    fn returns_to_user(&self) -> bool;

    /// 当前 trap 是否由架构的临时用户 signal 返回入口触发。
    fn is_user_signal_return(&self) -> bool;

    /// trap 返回用户态时将激活的地址空间 token（RISC-V Sv39 下为 `satp` 编码）。
    fn return_address_space_token(&self) -> usize;

    #[inline]
    #[allow(unused)]
    fn returns_to_kernel(&self) -> bool {
        !self.returns_to_user()
    }
}

#[allow(unused)]
pub trait TrapFrameWrite {
    fn set_user_pc(&mut self, pc: usize);
    fn add_user_pc(&mut self, bytes: usize);
    fn set_user_sp(&mut self, sp: usize);
    fn set_user_entry_args(&mut self, _argc: usize, _argv: usize, _envp: usize) {}
    fn enter_user_signal_handler(&mut self, handler: usize, signal: usize);
    fn set_return_to_user(&mut self);
    fn set_return_to_kernel(&mut self);

    #[inline]
    #[allow(unused)]
    fn prepare_user_return(&mut self, entry_pc: usize, user_sp: usize) {
        self.set_user_pc(entry_pc);
        self.set_user_sp(user_sp);
        self.set_return_to_user();
    }
}

#[allow(unused)]
pub trait TrapAddressSpaceWrite {
    /// 设置 trap 返回路径要激活的地址空间 token。
    ///
    /// 具体 token 编码由当前 arch/mm 实现决定；RISC-V Sv39 下为 `satp` 编码值。
    fn set_return_address_space_token(&mut self, token: usize);

    /// 准备 trap 返回路径的地址空间恢复信息。
    #[inline]
    fn prepare_address_space_for_return(&mut self, token: usize) {
        self.set_return_address_space_token(token);
    }
}

#[allow(unused)]
pub trait TrapSyscallRead {
    fn syscall_args(&self) -> SyscallArgs;
    fn syscall_nr(&self) -> SyscallNumber;

    #[inline]
    #[allow(unused)]
    fn syscall_context(&self) -> SyscallPacket {
        SyscallPacket::new(self.syscall_nr(), self.syscall_args())
    }
}

#[allow(unused)]
pub trait TrapSyscallWrite {
    fn set_syscall_ret(&mut self, ret: UserRet);
}

/// 线程相关 trap 帧写入接口。
///
/// 目前仅暴露用户态 TLS 寄存器写入，用于 `clone(CLONE_SETTLS)` 初始化新线程。
#[allow(unused)]
pub trait TrapThreadWrite {
    fn set_user_tls(&mut self, tls: usize);
}

/// 架构 trap 上下文的**只读**语义视图（兼容层）。
///
/// 新实现应直接实现 [`TrapFrameRead`] 与 [`TrapSyscallRead`]；本 trait 由 blanket
/// impl 自动提供，便于旧任务代码统一约束。
#[allow(unused)]
pub trait TrapContextRead: TrapFrameRead + TrapSyscallRead {
    #[inline]
    fn raw_cause(&self) -> usize {
        TrapFrameRead::raw_cause(self)
    }

    #[inline]
    fn trap_cause(&self) -> TrapCause {
        TrapFrameRead::trap_cause(self)
    }

    #[inline]
    fn fault_addr(&self) -> usize {
        TrapFrameRead::fault_addr(self)
    }

    #[inline]
    fn user_pc(&self) -> usize {
        TrapFrameRead::user_pc(self)
    }

    #[inline]
    fn user_sp(&self) -> usize {
        TrapFrameRead::user_sp(self)
    }

    #[inline]
    fn returns_to_user(&self) -> bool {
        TrapFrameRead::returns_to_user(self)
    }

    #[inline]
    fn returns_to_kernel(&self) -> bool {
        TrapFrameRead::returns_to_kernel(self)
    }

    #[inline]
    fn syscall_args(&self) -> SyscallArgs {
        TrapSyscallRead::syscall_args(self)
    }

    #[inline]
    fn syscall_nr(&self) -> SyscallNumber {
        TrapSyscallRead::syscall_nr(self)
    }

    #[inline]
    fn syscall_context(&self) -> SyscallPacket {
        TrapSyscallRead::syscall_context(self)
    }
}

impl<T: TrapFrameRead + TrapSyscallRead + ?Sized> TrapContextRead for T {}

/// 架构 trap 上下文的**可变**语义视图（兼容层）。
///
/// 将帧控制写回与系统调用返回值写入组合在同一约束中，具体类型仍分别实现
/// [`TrapFrameWrite`] 与 [`TrapSyscallWrite`]。
#[allow(unused)]
pub trait TrapContextWrite: TrapFrameWrite + TrapSyscallWrite + TrapAddressSpaceWrite {
    #[inline]
    fn set_syscall_ret(&mut self, ret: UserRet) {
        TrapSyscallWrite::set_syscall_ret(self, ret);
    }

    #[inline]
    fn set_user_pc(&mut self, pc: usize) {
        TrapFrameWrite::set_user_pc(self, pc);
    }

    #[inline]
    fn add_user_pc(&mut self, bytes: usize) {
        TrapFrameWrite::add_user_pc(self, bytes);
    }

    #[inline]
    fn set_user_sp(&mut self, sp: usize) {
        TrapFrameWrite::set_user_sp(self, sp);
    }

    #[inline]
    fn set_user_entry_args(&mut self, argc: usize, argv: usize, envp: usize) {
        TrapFrameWrite::set_user_entry_args(self, argc, argv, envp);
    }

    #[inline]
    fn set_return_to_user(&mut self) {
        TrapFrameWrite::set_return_to_user(self);
    }

    #[inline]
    fn set_return_to_kernel(&mut self) {
        TrapFrameWrite::set_return_to_kernel(self);
    }

    #[inline]
    fn set_return_address_space_token(&mut self, token: usize) {
        TrapAddressSpaceWrite::set_return_address_space_token(self, token);
    }

    #[inline]
    fn prepare_address_space_for_return(&mut self, token: usize) {
        TrapAddressSpaceWrite::prepare_address_space_for_return(self, token);
    }

    #[inline]
    #[allow(unused)]
    fn prepare_user_return(&mut self, entry_pc: usize, user_sp: usize) {
        TrapFrameWrite::prepare_user_return(self, entry_pc, user_sp);
    }
}

impl<T: TrapFrameWrite + TrapSyscallWrite + TrapAddressSpaceWrite + ?Sized> TrapContextWrite for T {}

#[allow(unused)]
pub trait TrapFrame: TrapContextRead + TrapContextWrite {}

impl<T: TrapContextRead + TrapContextWrite + ?Sized> TrapFrame for T {}

/// 历史拼写错误的 trait 名别名（仅兼容保留）。
#[allow(unused)]
#[deprecated(note = "use TrapContextWrite instead")]
pub trait TrapCOntextWrite: TrapContextWrite {}

#[allow(deprecated)]
impl<T: TrapContextWrite + ?Sized> TrapCOntextWrite for T {}

#[allow(unused)]
#[deprecated(note = "use TrapFrame instead")]
pub trait TrapContextFrameView: TrapContextRead + TrapContextWrite {}

#[allow(deprecated)]
impl<T: TrapContextRead + TrapContextWrite + ?Sized> TrapContextFrameView for T {}

/// 任务系统可保存、恢复并做语义读写的 trap 帧标记 trait。
///
/// 寄存器布局由当前 `arch-impl` 决定；task 组件只依赖此处语义契约，不在自身 API
/// 中重复架构相关的帧布局细节。
pub trait ArchTrapFrame:
    TrapContextRead + TrapContextWrite + Clone + Copy + core::fmt::Debug + Default + PartialEq + Eq
{
}

impl<T> ArchTrapFrame for T where
    T: TrapContextRead
        + TrapContextWrite
        + Clone
        + Copy
        + core::fmt::Debug
        + Default
        + PartialEq
        + Eq
{
}

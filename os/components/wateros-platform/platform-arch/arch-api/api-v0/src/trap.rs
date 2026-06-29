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
/// 同步异常语义（跨架构统一枚举；原始 CSR 解码由各 `arch-impl` 完成）。
#[derive(Clone, Copy, Debug)]
pub enum Exception {
    /// 用户态环境调用（系统调用入口）。
    UserEnvCall,
    /// 取指页故障。
    InstructionPageFault,
    /// 加载页故障。
    LoadPageFault,
    /// 存储页故障。
    StorePageFault,
    /// 非法指令。
    IllegalInstruction,
    /// 断点。
    Breakpoint,
    /// 当前 arch-api 未建模的异常码。
    Unsupported(usize),
}
#[allow(unused)]
/// 中断语义（跨架构统一枚举）。
#[derive(Clone, Copy, Debug)]
pub enum Interrupt {
    /// 监管态定时器中断。
    SupervisiorTimer,
    /// 监管态外部中断。
    SupervisiorExternel,
    /// 监管态软件中断。
    SupervisiorSoft,
    /// 当前 arch-api 未建模的中断码。
    Unsupported(usize),
}
#[allow(unused)]
/// trap 原因：异常或中断之一。
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
    /// 架构相关的 trap 原因原始编码（如 RISC-V `scause`）。
    fn raw_cause(&self) -> usize;

    /// 将 [`raw_cause`](Self::raw_cause) 解码为跨架构语义 [`TrapCause`]。
    fn trap_cause(&self) -> TrapCause;

    /// 故障地址（页故障等；无则实现可返回 0）。
    fn fault_addr(&self) -> usize;
    /// 用户态程序计数器。
    fn user_pc(&self) -> usize;
    /// 用户态栈指针。
    fn user_sp(&self) -> usize;
    /// 本次 trap 返回路径是否回到用户态。
    fn returns_to_user(&self) -> bool;

    /// 线程局部存储指针（LoongArch `$r2` / RISC-V `tp`）。
    #[inline]
    #[allow(unused)]
    fn user_tls(&self) -> usize {
        0
    }

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
    /// 设置用户态 PC。
    fn set_user_pc(&mut self, pc: usize);
    /// 将用户态 PC 前移 `bytes` 字节（跳过已模拟的指令）。
    fn add_user_pc(&mut self, bytes: usize);
    /// 设置用户态栈指针。
    fn set_user_sp(&mut self, sp: usize);
    /// 设置用户程序入口栈布局（`argc`/`argv`/`envp`）；默认无操作。
    fn set_user_entry_args(&mut self, _argc: usize, _argv: usize, _envp: usize) {}
    /// 标记 trap 返回路径回到用户态。
    fn set_return_to_user(&mut self);
    /// 标记 trap 返回路径留在内核态。
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
    /// 系统调用参数寄存器组。
    fn syscall_args(&self) -> SyscallArgs;
    /// 系统调用号。
    fn syscall_nr(&self) -> SyscallNumber;

    #[inline]
    #[allow(unused)]
    fn syscall_context(&self) -> SyscallPacket {
        SyscallPacket::new(self.syscall_nr(), self.syscall_args())
    }
}

#[allow(unused)]
pub trait TrapSyscallWrite {
    /// 写入系统调用返回值到 trap 帧。
    fn set_syscall_ret(&mut self, ret: UserRet);
}

/// 线程相关 trap 帧写入接口。
///
/// 目前仅暴露用户态 TLS 寄存器写入，用于 `clone(CLONE_SETTLS)` 初始化新线程。
#[allow(unused)]
pub trait TrapThreadWrite {
    /// 设置用户态 TLS 寄存器（`clone(CLONE_SETTLS)` 等）。
    fn set_user_tls(&mut self, tls: usize);
}

/// 用户态信号帧中保存的、与架构无关的最小机器上下文子集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct SignalMachineContext {
    /// 通用寄存器组（布局由具体 ISA 的 [`SignalFrameCodec`] 解释）。
    pub gprs: [usize; 32],
    /// 程序计数器。
    pub pc: usize,
    /// 特权/状态字（如 `sstatus`、LoongArch `prmd` 的用户态子集）。
    pub status: usize,
    /// 浮点寄存器快照。
    pub fpregs: [u64; 32],
    /// 浮点控制状态。
    pub fcsr: u32,
    /// 对齐填充，保留。
    pub reserved: u32,
}

impl Default for SignalMachineContext {
    fn default() -> Self {
        Self {
            gprs: [0; 32],
            pc: 0,
            status: 0,
            fpregs: [0; 32],
            fcsr: 0,
            reserved: 0,
        }
    }
}

/// 各支持的用户态 ISA 对信号帧寄存器的编解码接口。
pub trait SignalFrameCodec {
    /// 从当前 trap 帧捕获可写入用户信号帧的上下文。
    fn capture_signal_context(&self) -> SignalMachineContext;

    /// 仅恢复用户态合法的状态；上下文畸形时返回 `false`。
    fn restore_signal_context(&mut self, context: &SignalMachineContext) -> bool;

    /// 在用户栈上布置信号处理函数入口帧（handler、restorer、siginfo 等）。
    fn prepare_signal_handler(
        &mut self,
        handler: usize,
        restorer: usize,
        frame_sp: usize,
        signal: usize,
        siginfo: usize,
        ucontext: usize,
    );

    /// 改写已保存的用户上下文，使 `rt_sigreturn` 后以原参数重入被中断的系统调用。
    fn prepare_syscall_restart(
        context: &mut SignalMachineContext,
        syscall_nr: usize,
        args: [usize; 6],
        instruction_bytes: usize,
    );
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

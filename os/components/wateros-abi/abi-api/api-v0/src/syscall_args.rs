//! 系统调用参数在寄存器/陷阱帧与内核分发层之间的 C 布局承载形式。
//!
//! 槽位数量由依赖包 `wateros-base-config` 中的 `config::syscall::MAX_SYSCALL_ARGS` 决定，
//! 与 ABI 约定的参数个数上限一致。
//!
//! English: `repr(C)` carriers sized by `config::syscall::MAX_SYSCALL_ARGS` so trap
//! frames and the syscall dispatcher share one argument-cap upper bound.

use config::syscall::MAX_SYSCALL_ARGS;

use crate::syscall_number::SyscallNumber;

/// 一次系统调用的参数槽位数组（按 ABI 顺序对应各参数寄存器）。
///
/// 不解释各槽位语义；由具体调用号与内核 handler 约定含义。
///
/// English: raw argument words; meaning is defined per syscall number and handler.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SyscallArgs {
    /// 各参数槽位，索引越界属于调用方错误；顺序与 ABI 参数寄存器一致。
    ///
    /// English: argument slots in ABI register order; out-of-range indexing is a
    /// caller bug and must not be relied on for safety.
    pub args : [usize; config::syscall::MAX_SYSCALL_ARGS],
}

impl SyscallArgs {
    /// 由陷阱帧或寄存器快照构造参数包；不校验与 `nr` 是否匹配。
    ///
    /// English: builds from a register snapshot; does not validate syscall arity.
    #[inline]
    #[allow(unused)]
    pub const fn from_regs(regs : [usize; MAX_SYSCALL_ARGS]) -> Self {
        Self { args : regs }
    }

    /// 按槽位索引读取参数；`idx` 须在有效范围内。
    ///
    /// English: indexed read; caller must keep `idx` within `MAX_SYSCALL_ARGS`.
    #[inline]
    #[allow(unused)]
    pub const fn arg(&self, idx : usize) -> usize {
        self.args[idx]
    }

    /// 以固定长度数组形式取出全部槽位，供寄存器写回等场景使用。
    ///
    /// English: returns the full word array for register write-back paths.
    #[inline]
    #[allow(unused)]
    pub const fn as_regs(&self) -> [usize; config::syscall::MAX_SYSCALL_ARGS] {
        self.args
    }
}

/// 一次完整的用户态系统调用请求：调用号与参数包。
///
/// English: syscall number plus argument bundle for one user invocation.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SyscallPacket {
    /// 要分发的系统调用编号。
    ///
    /// English: syscall number to dispatch; not validated against `args` here.
    pub nr : SyscallNumber,
    /// 与该调用号对应的参数集合。
    ///
    /// English: argument bundle interpreted by the handler for `nr`.
    pub args : SyscallArgs,
}

impl SyscallPacket {
    /// 构造请求；不检查 `nr` 与 `args` 是否语义一致。
    ///
    /// English: pairs number and args without validating arity or types.
    #[inline]
    #[allow(unused)]
    pub const fn new(nr : SyscallNumber, args : SyscallArgs) -> Self {
        Self { nr : nr,
               args : args }
    }
}

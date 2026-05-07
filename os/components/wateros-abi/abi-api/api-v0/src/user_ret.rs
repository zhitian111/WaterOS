//! 用户态可见的系统调用返回值编码（成功非负、失败为负 errno）。

use crate::errno::ErrNo;

/// 用户态 ABI 可见的返回值类型。
///
/// Linux/riscv64 的约定是：成功时返回非负值；失败时返回 `-errno`（通常通过
/// `isize` 表示）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(C)]
#[allow(unused)]
pub struct UserRet(
    /// 已编码的返回值：非负为成功，负值为 `-errno`。
    pub isize,
);

/// 内核 handler 常用的结果类型：`Ok` 为成功时的非负整型结果（常为长度或句柄），`Err` 为 [`ErrNo`]。
pub type SyscallResult = core::result::Result<usize, ErrNo>;


impl UserRet {
    /// 成功路径：将非负成功值编码为 `UserRet`（直接按 `isize` 承载）。
    #[inline]
    pub const fn from_success(v : usize) -> UserRet { Self(v as isize) }

    /// 失败路径：编码为 `-errno`。
    #[inline]
    pub const fn from_error(errno : ErrNo) -> UserRet { Self(errno.user_ret()) }

    /// 将内核 `Result` 转为用户态可见的单一 `isize` 返回值。
    #[inline]
    pub fn from_kernel_result(res : SyscallResult) -> UserRet {
        match res {
            Ok(v) => Self::from_success(v),
            Err(e) => Self::from_error(e),
        }
    }
}

//! 用户态可见的系统调用返回值编码（成功非负、失败为负 errno）。
//!
//! English: encodes Linux-style syscall results as non-negative success or negative
//! errno for failures.

use crate::errno::ErrNo;

/// 用户态 ABI 可见的返回值类型。
///
/// Linux/riscv64 的约定是：成功时返回非负值；失败时返回 `-errno`（通常通过
/// `isize` 表示）。
///
/// English: thin newtype over `isize` following the Linux syscall return convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(C)]
#[allow(unused)]
pub struct UserRet(
    /// 已编码的返回值：非负为成功，负值为 `-errno`。
    ///
    /// English: encoded `isize`: non-negative success, negative errno magnitude.
    pub isize,
);

/// 内核 handler 常用的结果类型：`Ok` 为成功时的非负整型结果（常为长度或句柄），`Err` 为 [`ErrNo`]。
///
/// English: kernel-side `Result` before encoding to a single user-visible `isize`.
pub type SyscallResult = core::result::Result<usize, ErrNo>;


impl UserRet {
    /// 成功路径：将非负成功值编码为 `UserRet`（直接按 `isize` 承载）。
    ///
    /// English: success path; caller must ensure the value fits the syscall contract.
    #[inline]
    pub const fn from_success(v : usize) -> UserRet { Self(v as isize) }

    /// 失败路径：编码为 `-errno`。
    ///
    /// English: failure path using negative errno encoding.
    #[inline]
    pub const fn from_error(errno : ErrNo) -> UserRet { Self(errno.user_ret()) }

    /// 将内核 `Result` 转为用户态可见的单一 `isize` 返回值。
    ///
    /// English: maps `Ok` to a non-negative payload and `Err` to `-errno`.
    #[inline]
    pub fn from_kernel_result(res : SyscallResult) -> UserRet {
        match res {
            // 成功：保留非负 usize 载荷。 / Success: keep non-negative payload.
            Ok(v) => Self::from_success(v),
            // 失败：编码为负 errno。 / Failure: encode as negative errno.
            Err(e) => Self::from_error(e),
        }
    }
}

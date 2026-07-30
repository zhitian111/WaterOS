//! 用户态可见的系统调用返回值编码（成功非负、失败为负 errno）。

use crate::errno::{ErrNo, KernelResult};

/// 用户态 ABI 可见的返回值类型。
///
/// Linux generic 64 约定：成功时返回非负值；失败时返回 `-errno`
///（通常通过 `isize` 表示）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
pub struct UserRet(
    /// 已编码的返回值：非负为成功，负值为 `-errno`。
    pub isize,
);

impl UserRet {
    /// 成功路径：将非负成功值编码为 `UserRet`（直接按 `isize` 承载）。
    #[inline]
    pub const fn from_success(v : usize) -> UserRet { Self(v as isize) }

    /// 失败路径：编码为 `-errno`。
    #[inline]
    pub const fn from_error(errno : ErrNo) -> UserRet { Self(errno.user_ret()) }

    /// ABI_CONTRACT: 将未编码的内核结果转为用户态可见的单一 `isize`。
    ///
    /// `Ok` 载荷通常为长度、文件描述符或地址；调用方需保证它能按 ABI
    /// 表示为非负 `isize`。`ErrNo` 始终在此处变为 `-errno`。
    #[inline]
    pub fn from_kernel_result(res : KernelResult<usize>) -> UserRet {
        match res {
            Ok(v) => Self::from_success(v),
            Err(e) => Self::from_error(e),
        }
    }
}

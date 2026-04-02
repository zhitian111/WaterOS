use crate::errno::ErrNo;

/// 用户态 ABI 可见的返回值类型。
///
/// Linux/riscv64 的约定是：成功时返回非负值；失败时返回 `-errno`（通常通过
/// `isize` 表示）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(C)]
#[allow(unused)]
pub struct UserRet(pub isize);

pub type SyscallResult = core::result::Result<usize, ErrNo>;


impl UserRet {
    #[inline]
    pub const fn from_success(v : usize) -> UserRet { Self(v as isize) }
    #[inline]
    pub const fn from_error(errno : ErrNo) -> UserRet { Self(errno.user_ret()) }
    #[inline]
    pub fn from_kernel_result(res : SyscallResult) -> UserRet {
        match res {
            Ok(v) => Self::from_success(v),
            Err(e) => Self::from_error(e),
        }
    }
}

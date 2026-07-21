//! 系统调用编号的类型抽象。

/// Linux/riscv64 系统调用号 newtype（只表示编号，不在类型层做合法性校验）。
#[repr(transparent)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SyscallNumber(
    /// 平台 ABI 下的裸系统调用编号。
    pub usize,
);

impl SyscallNumber {
    /// 由裸编号构造；不检查该编号在当前内核是否受支持。
    #[inline]
    pub const fn new(n: usize) -> Self {
        Self(n)
    }

    /// 取底层 `usize` 调用号。
    #[inline]
    pub const fn raw(self) -> usize {
        self.0
    }
}

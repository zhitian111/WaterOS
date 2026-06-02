//! Futex 队列键：按用户态地址与 private 标志区分等待队列。

/// Linux `futex(2)` 操作码中的 private 标志位。
pub const FUTEX_PRIVATE_FLAG: u32 = 128;

/// 由用户 futex 地址与 private 标志派生的队列键。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FutexKey {
    /// 用户态 futex 字地址。
    pub uaddr: usize,
    /// 是否为进程私有 futex（`FUTEX_PRIVATE_FLAG`）。
    pub is_private: bool,
}

impl FutexKey {
    /// 从 futex 用户地址构造队列键（兼容仅按地址建键的旧路径）。
    #[inline]
    pub const fn from_uaddr(uaddr: usize) -> Self {
        Self {
            uaddr,
            is_private: true,
        }
    }

    /// 从 syscall 参数解析队列键。
    #[inline]
    pub const fn from_syscall(uaddr: usize, futex_op: u32) -> Self {
        Self {
            uaddr,
            is_private: (futex_op & FUTEX_PRIVATE_FLAG) != 0,
        }
    }
}

//! Futex 队列键：按用户态地址区分等待队列。

/// 由用户态 futex 变量地址派生的队列键。
///
/// 同页内不同 futex 变量必须映射到不同键，避免错误唤醒。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FutexKey(pub usize);

impl FutexKey {
    /// 从 futex 用户地址构造队列键。
    #[inline]
    pub const fn from_uaddr(uaddr : usize) -> Self { Self(uaddr) }

    /// 返回底层用户地址。
    #[inline]
    pub const fn uaddr(self) -> usize { self.0 }
}

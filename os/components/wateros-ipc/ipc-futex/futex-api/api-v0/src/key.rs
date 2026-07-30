//! Futex 队列键：private futex 按地址空间隔离，shared futex 保持全局可见。

/// Linux `futex(2)` 操作码中的 private 标志位。
pub const FUTEX_PRIVATE_FLAG : u32 = 128;

/// DATA: 由用户 futex 地址与作用域派生的队列键。
///
/// ABI: private key 的相等性包含地址空间 scope；shared key 的 `uaddr` 必须是
/// MM 解析出的稳定共享身份，而不是任意进程的用户虚拟地址。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FutexKey {
    /// private futex 为用户 VA；shared futex 为 MM 解析出的共享字身份。
    pub uaddr : usize,
    /// 是否为进程私有 futex（`FUTEX_PRIVATE_FLAG`）。
    pub is_private : bool,
    /// private futex 所属地址空间；shared futex 固定为 0。
    pub private_scope : usize,
}

impl FutexKey {
    /// 构造属于指定地址空间的 private futex 键。
    #[inline]
    pub const fn private(uaddr : usize, private_scope : usize) -> Self {
        Self { uaddr,
               is_private : true,
               private_scope }
    }

    /// 使用 MM 已解析的稳定共享字身份构造 shared futex 键。
    #[inline]
    pub const fn shared(shared_identity : usize) -> Self {
        Self { uaddr : shared_identity,
               is_private : false,
               private_scope : 0 }
    }

    /// 从 syscall 参数解析队列键。
    ///
    /// 该便捷接口只适合测试 private flag 的解析；生产 syscall 对 shared
    /// futex 必须先通过 MM 把 VA 解析为共享映射身份，再调用 [`Self::shared`]。
    #[inline]
    pub const fn from_syscall(uaddr : usize, futex_op : u32, private_scope : usize) -> Self {
        if futex_op & FUTEX_PRIVATE_FLAG != 0 {
            Self::private(uaddr, private_scope)
        } else {
            Self::shared(uaddr)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{FutexKey, FUTEX_PRIVATE_FLAG};

    #[test]
    fn private_keys_are_scoped_by_address_space() {
        let a = FutexKey::from_syscall(0x1000, FUTEX_PRIVATE_FLAG, 1);
        let b = FutexKey::from_syscall(0x1000, FUTEX_PRIVATE_FLAG, 2);
        assert_ne!(a, b);
    }

    #[test]
    fn shared_keys_ignore_private_scope() {
        let a = FutexKey::from_syscall(0x1000, 0, 1);
        let b = FutexKey::from_syscall(0x1000, 0, 2);
        assert_eq!(a, b);
    }
}

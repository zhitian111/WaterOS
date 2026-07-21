//! 由自旋互斥锁保护的多核全局可变容器。

use spin::{Mutex, MutexGuard};

/// 可在多个 CPU 间共享并独占修改的全局容器。
pub struct MultiprocessorSafeCell<T> {
    inner : Mutex<T>,
}

impl<T> MultiprocessorSafeCell<T> {
    pub const fn new(value : T) -> Self { Self { inner : Mutex::new(value) } }

    /// 获取跨核独占 guard；guard 释放时自动解锁。
    pub fn exclusive_access(&self) -> MutexGuard<'_, T> { self.inner.lock() }

    /// `exclusive_access` 的常用锁语义别名。
    pub fn lock(&self) -> MutexGuard<'_, T> { self.inner.lock() }
}

#[cfg(test)]
mod tests {
    use super::MultiprocessorSafeCell;

    #[test]
    fn mutation_is_visible_after_unlock() {
        let value = MultiprocessorSafeCell::new(1);
        *value.exclusive_access() += 1;
        assert_eq!(*value.lock(), 2);
    }
}

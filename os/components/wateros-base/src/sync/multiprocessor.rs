//! 由自旋互斥锁保护的多核全局可变容器。

use spin::{Mutex, MutexGuard};

/// 可在多个 CPU 间共享并独占修改的全局容器。
///
/// 它只提供互斥，不管理中断状态，也不允许同一 CPU 重入。可能在中断上下文访问
/// 的调用方必须先关闭本地中断，避免中断处理程序再次获取同一把锁。
pub struct MultiprocessorSafeCell<T> {
    /// 保护内部值的自旋锁；锁不负责屏蔽中断或防止递归获取。
    inner : Mutex<T>,
}

impl<T> MultiprocessorSafeCell<T> {
    /// 以 `value` 构造一个未上锁的容器，可用于静态初始化。
    pub const fn new(value : T) -> Self { Self { inner : Mutex::new(value) } }

    /// 获取跨核独占 guard；guard 离开作用域时自动解锁。
    ///
    /// 这是阻塞式自旋操作。不得持有该 guard 调度、睡眠或进入可能重入本锁的
    /// 跨层回调。
    pub fn exclusive_access(&self) -> MutexGuard<'_, T> {
        // 自旋锁在持有期间不能阻塞或调度；调用方必须把临界区限制在短小的状态更新内。
        self.inner.lock()
    }

    /// 尝试获取跨核独占 guard；锁已被持有时立即返回 `None`。
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        self.inner
            .try_lock()
    }
}

#[cfg(test)]
mod tests {
    use super::MultiprocessorSafeCell;

    #[test]
    fn mutation_is_visible_after_unlock() {
        let value = MultiprocessorSafeCell::new(1);
        *value.exclusive_access() += 1;
        assert_eq!(*value.exclusive_access(), 2);
    }

    #[test]
    fn try_lock_reports_contention() {
        let value = MultiprocessorSafeCell::new(1);
        let guard = value.exclusive_access();
        assert!(value.try_lock()
                     .is_none());
        drop(guard);
        assert!(value.try_lock()
                     .is_some());
    }
}

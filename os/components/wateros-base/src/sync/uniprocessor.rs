//! 单核（或等价互斥初始化）假设下，用 `RefCell` 在运行时提供对全局 `T` 的可变借用出口。
//!
//! 不提供自旋锁；跨 hart 或抢占重入场景须由调用方另行同步。

use core::cell::{RefCell, RefMut};

/// 单核环境下的“安全单例容器”：
/// - 通过 `RefCell` 在运行时保证 `exclusive_access()` 产生的 `&mut T` 唯一性；
/// - 适用于你这种在 `impl-*` 中维护全局分配器实例的场景。
pub struct UniprocessorSafeCell<T> {
    // 独占可变访问的运行时闸门；违反借用规则时在 `borrow_mut` 处 panic。
    inner: RefCell<T>,
}

// 单核/单线程假设下该容器用于跨模块共享；安全性由调用约束与 RefCell 运行时借用规则保证。
unsafe impl<T> Sync for UniprocessorSafeCell<T> {}

impl<T> UniprocessorSafeCell<T> {
    /// 构造容器；调用方需保证仅在单核/可互斥访问的初始化路径上调用，避免并发 `new`。
    pub unsafe fn new(value: T) -> Self {
        Self { inner: RefCell::new(value) }
    }

    /// 获取对内部值的独占可变借用；若已存在未释放的借用则会在运行时 panic。
    pub fn exclusive_access(&self) -> RefMut<'_, T> {
        self.inner.borrow_mut()
    }
}


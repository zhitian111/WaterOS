//! 内核堆后端抽象：TLSF/链表后端通过同一接口接入，后续 slab 后端也实现该接口。
//!
//! 当前 crate 在编译期按 feature 选择唯一活动后端；运行期切换能力由 Task 03
//! 在 [`crate::KernelAllocator`] 之上补充。

use core::alloc::Layout;

use crate::HeapMemStats;

/// 内核堆后端必须满足的操作集合。
///
/// 所有方法都假设调用方已经满足 `ALLOC_SYNC`/`RUNTIME_ORDER` 不变量；具体后端内部
/// 继续使用 [`crate::interrupt_guard`] 或自己的同步原语。
pub(crate) trait HeapBackend : Sync {
    /// 初始化后端。只能在 BSP 单线程引导阶段调用一次。
    fn init(&self);

    /// 返回当前后端的内存统计快照。
    fn mem_stats(&self) -> HeapMemStats;

    /// 按 `layout` 分配内存；失败返回 null。
    ///
    /// # Safety
    /// 与 [`core::alloc::GlobalAlloc::alloc`] 相同的调用约束。
    unsafe fn alloc(&self, layout : Layout) -> *mut u8;

    /// 释放由 [`Self::alloc`]/[`Self::realloc`] 返回的指针。
    ///
    /// # Safety
    /// 与 [`core::alloc::GlobalAlloc::dealloc`] 相同的调用约束。
    unsafe fn dealloc(&self, ptr : *mut u8, layout : Layout);

    /// 调整分配大小。
    ///
    /// # Safety
    /// 与 [`core::alloc::GlobalAlloc::realloc`] 相同的调用约束。
    unsafe fn realloc(&self,
                      ptr : *mut u8,
                      layout : Layout,
                      new_size : usize)
                      -> *mut u8;
}

//! 物理帧分配器聚合：按 feature 导出 API 与 **栈式**或 **dummy** 实现；为 `mm-impl` 页表与内核 bring-up 提供 `PhysPageNum` 粒度的帧。

#![no_std]

pub use api_v0::*;

#[cfg(feature = "impl-stack")]
pub use impl_stack::*;

#[cfg(feature = "impl-dummy")]
pub use impl_dummy::*;

/// 帧分配器自测：`BasePPN` 表示 **物理页号** 闭开区间 `[start, end)`，与内核传给 `init_frame_allocator` 的区间一致。
pub fn test_with_range(start_ppn: wateros_base::addr::BasePPN, end_ppn: wateros_base::addr::BasePPN) {
    log::trace!("[frame-alloctor] test begin");
    #[cfg(feature = "impl-stack")]
    impl_stack::test_with_range(start_ppn, end_ppn);
    #[cfg(feature = "impl-dummy")]
    {
        let _ = (start_ppn, end_ppn);
        log::info!("[frame-alloctor] dummy impl: no test");
    }
    log::trace!("[frame-alloctor] test end");
}

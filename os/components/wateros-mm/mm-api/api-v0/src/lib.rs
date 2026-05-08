//! MM API v0：虚拟/物理地址、页权限、地址空间与 mmap/brk/用户访问等 **trait 契约**。
//!
//! 本 crate 不实现具体页表；**4 KiB 页** 与地址分解见 [`addr`]。实现侧（如 Sv39）须与这里的语义一致，并在文档中写明平台假设（恒等映射、trap 入口映射等）。

#![no_std]

pub mod addr;
pub mod error;
pub mod perm;
pub mod flags;

pub mod frame_allocator;
pub mod address_space;
pub mod user_access;

pub mod brk;
pub mod mmap;
pub mod kernel_bringup;

pub use frame_allocator::PhysicalFrameAllocator;

/// 聚合自测：地址分解、权限与 mmap 标志；不分配真实物理帧。
pub fn test() {
    log::trace!("[mm-api] test begin");
    addr::test();
    perm::test();
    flags::test();
    log::trace!("[mm-api] test end");
}

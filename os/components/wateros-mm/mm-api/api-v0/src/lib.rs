//! MM API v0：虚拟/物理地址、页权限、地址空间与 mmap/brk/用户访问等 **trait
//! 契约**。
//!
//! 本 crate 不实现具体页表；**4 KiB 页** 与地址分解见 [`addr`]。实现侧（如
//! Sv39）须与这里的语义一致，并在文档中写明平台假设（恒等映射、trap
//! 入口映射等）。**不**依赖文件系统 API crate；根卷读错误见
//! [`kernel_bringup::RootVolumeReadError`]。

#![no_std]

extern crate alloc;

pub mod addr;
pub mod error;
pub mod flags;
pub mod perm;

pub mod address_space;
pub mod frame_allocator;
pub mod user_access;

pub mod brk;
pub mod elf_user_stack;
pub mod executable;
pub mod kernel_bringup;
pub mod kernel_satp;
pub mod mempolicy;
pub mod mmap;
pub mod user_aspace_lifecycle;
pub mod user_mapping;

pub use frame_allocator::PhysicalFrameAllocator;

/// 聚合自测：地址分解、权限与 mmap 标志；不分配真实物理帧。
pub fn test() {
    log::trace!("[mm-api] test begin");
    addr::test();
    perm::test();
    flags::test();
    executable::test();
    user_access::test();
    log::trace!("[mm-api] test end");
}

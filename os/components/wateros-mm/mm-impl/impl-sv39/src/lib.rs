//! RISC-V **Sv39** 内存管理实现 crate：页表细节见内部 [`pagetable`]，对外仅通过 [`kernel_mm_impl`] 与自检入口暴露。
//!
//! 页表 walk、PTE 编码等 **不** 作为本 crate 根模块公共 API，避免与 [`api_v0::address_space::AddressSpaceOps`] 契约重复暴露。

#![no_std]

extern crate alloc;

use api_v0::addr::{VirtAddr, VirtPageNum, PAGE_SIZE};
use api_v0::address_space::AddressSpaceOps;
use api_v0::error::MmError;
use api_v0::perm::PagePerm;

use frame_alloctor::{frame_alloc_result, frame_dealloc_result};
use wateros_base::addr::BasePPN;

mod pagetable;

mod kernel_elf;
mod kernel_global;
mod user_heap_mmap;
pub mod user_access;
pub mod user_aspace;

/// Sv39 页表 walk、映射/解映射/权限与翻译的自测；依赖已初始化的全局帧分配器（区间语义同 bring-up）。
pub fn test_with_range(start_ppn: BasePPN, end_ppn: BasePPN) {
    log::trace!("[mm-impl::sv39] test begin");
    frame_alloctor::test_with_range(start_ppn, end_ppn);

    let mut aspace = pagetable::Sv39AddressSpace::new().expect("Sv39AddressSpace::new should succeed");
    let satp = aspace.satp_value();
    assert_eq!(satp >> 60, 8, "satp mode should be Sv39");

    let ppn = frame_alloc_result().expect("alloc one frame for map test");
    let vpn = VirtPageNum(0x200);
    let perm = PagePerm::R | PagePerm::W | PagePerm::U;
    aspace.map_page_to_ppn(vpn, ppn, perm).expect("map should succeed");
    let map_dup = aspace.map_page_to_ppn(vpn, ppn, perm);
    assert!(matches!(map_dup, Err(MmError::AlreadyMapped)));

    let va = VirtAddr(vpn.0 * PAGE_SIZE + 0x123);
    let pa = aspace.translate_addr(va).expect("translate should not error").expect("should map");
    assert_eq!(pa.0, ppn.0 * PAGE_SIZE + 0x123);

    aspace
        .protect_page(vpn, PagePerm::R | PagePerm::U)
        .expect("protect should succeed");
    let pa2 = aspace.translate_addr(va).unwrap().unwrap();
    assert_eq!(pa2.0, pa.0);

    let old = aspace.unmap_page_to_ppn(vpn).expect("unmap should succeed");
    assert_eq!(old, Some(ppn));
    let none = aspace.translate_addr(va).unwrap();
    assert!(none.is_none());
    let missing_protect = aspace.protect_page(vpn, PagePerm::R);
    assert!(matches!(missing_protect, Err(MmError::NotMapped)));
    let second_unmap = aspace.unmap_page_to_ppn(vpn).expect("second unmap should be ok");
    assert!(second_unmap.is_none());

    frame_dealloc_result(ppn).expect("dealloc test frame");

    log::trace!("[mm-impl::sv39] test end");
}

/// 内核全局页表与用户 ELF 装载（QEMU RISC-V bring-up）；由 `wateros-mm` 聚合为 `mm::kernel_mm`。
pub mod kernel_mm_impl {
    pub use crate::kernel_elf::{from_elf_bytes, from_elf_path};
    pub use crate::kernel_global::{
        ensure_user_execute_for_kernel_va, init, kernel_satp, map_anon_range_user,
        map_identity_range_user,
    };

    /// 基于父地址空间创建独立子地址空间：分配新页表树并逐帧复制所有带 `U`
    /// 权限的用户页。
    ///
    /// `parent_aspace_ptr` 来自 `LoadedElf::user_aspace_ptr` / `UserTask::user_aspace_ptr()`，
    /// 即 `Sv39AddressSpace` 的泄漏裸指针。
    ///
    /// 返回 `(子地址空间裸指针, 子 satp 编码值)`；`parent_aspace_ptr == 0` 时返回
    /// [`api_v0::error::MmError::InvalidAddress`]。
    pub fn fork_user_aspace(parent_aspace_ptr: usize) -> api_v0::error::MmResult<(usize, usize)> {
        use alloc::boxed::Box;
        use api_v0::address_space::AddressSpaceOps;
        use api_v0::error::MmError;
        use crate::pagetable::Sv39AddressSpace;

        if parent_aspace_ptr == 0 {
            return Err(MmError::InvalidAddress);
        }
        // SAFETY: 调用方保证 parent_aspace_ptr 指向一直存活（泄漏）的 Sv39AddressSpace
        let parent = unsafe { &*(parent_aspace_ptr as *const Sv39AddressSpace) };
        let child = parent.fork()?;
        let satp = child.satp_value();
        let child_ptr = Box::into_raw(Box::new(child)) as usize;
        Ok((child_ptr, satp))
    }
}

//! LoongArch64 **三级页表** 内存管理实现 crate：页表细节见内部
//! [`pagetable`]，对外仅通过 [`kernel_mm_impl`] 与自检入口暴露。
//!
//! 页表 walk、PTE 编码等 **不** 作为本 crate 根模块公共 API，避免与
//! [`api_v0::address_space::AddressSpaceOps`] 契约重复暴露。

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use api_v0::addr::{PhysPageNum, VirtAddr, VirtPageNum, PAGE_SIZE};
use api_v0::address_space::AddressSpaceOps;
use api_v0::error::{MmError, MmResult};
use api_v0::mmap::{DemandPageLoader, MmapOps, PageFaultAccess};
use api_v0::perm::PagePerm;

use frame_alloctor::{frame_alloc_result, frame_dealloc_result, GlobalPhysFrameAllocator};
mod asid;
mod pagetable;

mod kernel_elf;
mod kernel_executable;
mod kernel_global;
pub mod user_access;
pub mod user_aspace;
mod user_heap_mmap;

use pagetable::VmaBacking;

struct WritableFaultTestLoader;

impl DemandPageLoader for WritableFaultTestLoader {
    fn duplicate_box(&self) -> MmResult<Box<dyn DemandPageLoader>> {
        Ok(Box::new(WritableFaultTestLoader))
    }

    fn load_page(&mut self, _file_offset : usize, dst : &mut [u8]) -> MmResult<()> {
        dst[0] = 0x5a;
        Ok(())
    }

    fn load_shared_page(&mut self, _file_offset : usize) -> MmResult<Option<PhysPageNum>> {
        panic!("writable lazy fault must not request an immutable shared page")
    }
}

/// LoongArch64 三级页表 walk、映射/解映射/权限与翻译的自测；
/// 依赖已初始化的全局帧分配器（区间语义同 bring-up）。
pub fn test_with_range(start_ppn : PhysPageNum, end_ppn : PhysPageNum) {
    log::trace!("[mm-impl::loongarch64] test begin");
    frame_alloctor::test_with_range(start_ppn, end_ppn);

    let mut aspace =
        pagetable::LoongArch64AddressSpace::new().expect("LoongArch64AddressSpace::new should \
                                                          succeed");
    let _pgdl = aspace.satp_value();

    let ppn = frame_alloc_result().expect("alloc one frame for map test");
    let vpn = VirtPageNum(0x200);
    let perm = PagePerm::R | PagePerm::W | PagePerm::U;
    aspace.map_page_to_ppn(vpn, ppn, perm)
          .expect("map should succeed");
    let map_dup = aspace.map_page_to_ppn(vpn, ppn, perm);
    assert!(matches!(map_dup, Err(MmError::AlreadyMapped)));

    let va = VirtAddr(vpn.0 * PAGE_SIZE + 0x123);
    let pa = aspace.translate_addr(va)
                   .expect("translate should not error")
                   .expect("should map");
    assert_eq!(pa.0, ppn.0 * PAGE_SIZE + 0x123);

    let changed = MmapOps::mprotect(&mut aspace,
                                    vpn.start_addr(),
                                    PAGE_SIZE,
                                    PagePerm::R)
                      .expect("mprotect should succeed");
    assert!(changed, "resident permission change must require a flush");
    let unchanged = MmapOps::mprotect(&mut aspace,
                                      vpn.start_addr(),
                                      PAGE_SIZE,
                                      PagePerm::R)
                        .expect("same-permission mprotect should succeed");
    assert!(!unchanged, "same resident permission must not require a flush");
    let pa2 = aspace.translate_addr(va)
                    .unwrap()
                    .unwrap();
    assert_eq!(pa2.0, pa.0);

    let old = aspace.unmap_page_to_ppn(vpn)
                    .expect("unmap should succeed");
    assert_eq!(old, Some(ppn));
    let none = aspace.translate_addr(va)
                     .unwrap();
    assert!(none.is_none());
    let missing_protect = aspace.protect_page(vpn, PagePerm::R);
    assert!(matches!(missing_protect, Err(MmError::NotMapped)));
    let second_unmap = aspace.unmap_page_to_ppn(vpn)
                             .expect("second unmap should be ok");
    assert!(second_unmap.is_none());

    frame_dealloc_result(ppn).expect("dealloc test frame");

    let lazy_vpn = VirtPageNum(0x400);
    let lazy_start = lazy_vpn.start_addr();
    let lazy_end = VirtPageNum(lazy_vpn.0 + 1).start_addr();
    aspace.register_lazy_file_vma(lazy_start,
                                  lazy_end,
                                  PagePerm::R | PagePerm::U,
                                  0,
                                  0,
                                   VmaBacking::File { loader : Box::new(WritableFaultTestLoader) })
          .expect("register lazy page");
    let lazy_changed = MmapOps::mprotect(&mut aspace,
                                         lazy_start,
                                         PAGE_SIZE,
                                         PagePerm::R | PagePerm::W)
                           .expect("protect lazy page");
    assert!(!lazy_changed, "lazy VMA-only change must not require a flush");
    assert!(aspace.leaf_page_perm(lazy_vpn).unwrap().is_none());
    let mut allocator = GlobalPhysFrameAllocator;
    assert!(MmapOps::handle_page_fault(&mut aspace,
                                       &mut allocator,
                                       lazy_start,
                                       PageFaultAccess::Write)
            .expect("fault protected lazy page"));
    assert_eq!(aspace.leaf_page_perm(lazy_vpn).unwrap(),
               Some(PagePerm::R | PagePerm::W | PagePerm::U));
    let lazy_ppn = aspace.unmap_page_to_ppn(lazy_vpn)
                          .expect("unmap lazy test page")
                          .expect("lazy test page should be resident");
    frame_dealloc_result(lazy_ppn).expect("dealloc lazy test frame");
    impl_common::test_readonly_elf_page_cache();
    impl_common::test_readonly_mmap_page_cache();
    user_access::test_copy_to_user_progress();

    log::trace!("[mm-impl::loongarch64] test end");
}

/// LoongArch64 内核态用户拷贝自检；只操作临时地址空间，函数结束时释放页表和帧。
#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[mm-impl::loongarch64] self_test begin");
    user_access::test_copy_to_user_progress();
    log::info!("[mm-impl::loongarch64] self_test complete; temporary mappings reclaimed");
}

/// 内核全局页表与用户 ELF 装载（QEMU LoongArch64 bring-up）；由 `wateros-mm`
/// 聚合为 `mm::kernel_mm`。
pub mod kernel_mm_impl {
    pub use crate::kernel_elf::{from_elf_bytes, from_elf_path, read_path_bytes};
    pub use crate::kernel_executable::load_program_from_path;
    pub use crate::kernel_global::{
        ensure_user_execute_for_kernel_va, init, kernel_satp, map_anon_range_user,
        map_identity_range_user,
    };
    pub use crate::user_aspace::handle_tlb_shootdown_ipi;

    /// 基于父地址空间创建 COW 子地址空间：复制页表树，用户页共享到写时复制。
    ///
    /// `parent_aspace_ptr` 来自 `LoadedElf::user_aspace_ptr` /
    /// `UserTask::user_aspace_ptr()`， 即 `LoongArch64AddressSpace`
    /// 的泄漏裸指针。
    ///
    /// 返回 `(子地址空间裸指针, 子地址空间 token)`；`parent_aspace_ptr == 0`
    /// 时返回 [`api_v0::error::MmError::InvalidAddress`]。
    // 本方法代码由AI完成
    pub fn fork_user_aspace(parent_aspace_ptr : usize) -> api_v0::error::MmResult<(usize, usize)> {
        use api_v0::error::MmError;
        use api_v0::address_space::AddressSpaceOps;

        if parent_aspace_ptr == 0 {
            return Err(MmError::InvalidAddress);
        }
        let (child, pgdl) = crate::user_aspace::with_user_aspace_mut_and_flush(parent_aspace_ptr,
            |parent| {
                let child = parent.fork_cow()?;
                let pgdl = child.satp_value();
                Ok((child, pgdl))
            })?;
        Ok((crate::user_aspace::into_handle(child), pgdl))
    }

    // 本方法代码由AI完成
    pub fn handle_cow_fault(parent_aspace_ptr : usize,
                            fault_addr : usize)
                            -> api_v0::error::MmResult<bool> {
        use api_v0::addr::VirtAddr;
        use api_v0::error::MmError;

        if parent_aspace_ptr == 0 {
            return Err(MmError::InvalidAddress);
        }
        crate::user_aspace::with_user_aspace_mut_and_page_flush(parent_aspace_ptr,
            fault_addr,
            |aspace| {
                let changed = aspace.handle_cow_fault_no_flush(VirtAddr(fault_addr))?;
                Ok((changed, changed))
            })
    }

    /// 销毁用户地址空间：递归释放所有用户页帧和页表帧。
    ///
    /// `aspace_ptr` 来自 `LoadedElf::user_aspace_ptr`，调用后指针失效。
    // 本方法代码由AI完成
    pub fn drop_user_aspace(aspace_ptr : usize) {
        crate::user_aspace::destroy(aspace_ptr);
    }
}

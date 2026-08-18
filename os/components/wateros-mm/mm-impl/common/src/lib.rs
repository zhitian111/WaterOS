//! 各 arch `mm-impl` 共享的实现辅助逻辑（ELF 装载、mmap/mremap、按需零页等）。
//!
//! 本 crate **不**对外暴露稳定契约，位于 `wateros-mm-api-v0` 之下：可依赖当前
//! loader 策略与 bring-up 假设；语义边界仍以 `mm-api` 为准。

#![no_std]

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;

use api_v0::addr::{PhysPageNum, VirtAddr, VirtPageNum, PAGE_SIZE};
use core::cmp;
use api_v0::address_space::AddressSpaceOps;
use api_v0::error::{MmError, MmResult};
use api_v0::executable;
use api_v0::frame_allocator::PhysicalFrameAllocator;
use api_v0::kernel_bringup::LoadElfError;
use api_v0::mmap::DemandPageLoader;
use api_v0::perm::PagePerm;
use core::sync::atomic::AtomicU64;
use frame_alloctor::{frame_alloc_result, frame_dealloc_result, frame_inc_ref, frame_ref_count};
use vfs_api::VfsFileContentIdentity;

/// Convert a physical byte address to the address usable by kernel code.
/// LoongArch runs through the high cached DMW window; raw low physical
/// pointers are not valid after paging is enabled.
#[inline]
pub(crate) fn phys_access_addr(pa : usize) -> usize {
    #[cfg(target_arch = "loongarch64")]
    {
        let kernel_start : usize;
        unsafe { core::arch::asm!("la {}, kernel_start", out(reg) kernel_start); }
        (kernel_start & 0xFFFF_0000_0000_0000usize) | pa
    }
    #[cfg(not(target_arch = "loongarch64"))]
    { pa }
}

/// 私有匿名映射的惰性缺页 loader：缺页时不做任何加载，
/// 直接保留 `handle_lazy_page_fault` 预先清零的页（等价于按需零页）。
///
/// 复用文件 lazy VMA 机制，避免匿名 mmap 饥渴分配整段物理帧
/// （例如 glibc pthread 每线程 8 MiB 栈，批量创建会瞬间耗尽帧池 → `ENOMEM`）。
pub struct ZeroAnonLoader;

#[cfg(feature = "self_test")]
/// 运行共享 MM 辅助逻辑的最小自检；调用方必须先准备帧分配器和文件身份环境。
pub fn self_test() {
    log::info!("[mm/common] self_test begin");
    let mut page = [0xA5; 16];
    ZeroAnonLoader.load_page(0, &mut page).expect("zero loader must accept a page");
    assert_eq!(page, [0xA5; 16]);
    test_readonly_elf_page_cache();
    test_readonly_mmap_page_cache();
    log::info!("[mm/common] self_test complete");
}

impl DemandPageLoader for ZeroAnonLoader {
    /// 复制一个无状态匿名 loader，供 fork 后父子地址空间各自持有。
    fn duplicate_box(&self) -> MmResult<Box<dyn DemandPageLoader>> { Ok(Box::new(ZeroAnonLoader)) }

    fn mapping_kind(&self) -> api_v0::mmap::DemandMappingKind {
        api_v0::mmap::DemandMappingKind::Anonymous
    }

    /// 匿名页无需从文件读取；目标页应在调用前已经清零。
    fn load_page(&mut self, _file_offset : usize, _dst : &mut [u8]) -> MmResult<()> { Ok(()) }
}

/// PT_LOAD 惰性缺页：按页从 ELF 文件区间填充 `dst`（段前/BSS 由调用方预先清零）。
pub fn fill_elf_load_page<F>(vbase : usize,
                             p_offset : usize,
                             filesz : usize,
                             page_va : usize,
                             dst : &mut [u8],
                             mut read_file : F)
                             -> MmResult<()>
    where F : FnMut(usize, &mut [u8]) -> MmResult<()>
{
    let page_end = page_va.checked_add(dst.len())
                          .ok_or(MmError::InvalidAddress)?;
    let file_end_va = vbase.checked_add(filesz)
                           .ok_or(MmError::InvalidAddress)?;
    let seg_start = cmp::max(page_va, vbase);
    let seg_end = cmp::min(page_end, file_end_va);
    if seg_start >= seg_end {
        return Ok(());
    }
    let dst_off = seg_start - page_va;
    let rel = seg_start.checked_sub(vbase)
                       .ok_or(MmError::InvalidAddress)?;
    let len = seg_end - seg_start;
    let file_pos = p_offset.checked_add(rel)
                           .ok_or(MmError::InvalidAddress)?;
    read_file(file_pos, &mut dst[dst_off..dst_off + len])
}

/// execve 登记惰性映射 VMA 时的段参数（供各架构 `kernel_elf` 构造 loader）。
#[derive(Clone, Debug)]
pub struct ElfSegmentLoadParams {
    /// ELF 段在进程地址空间中的虚拟基址。
    pub vbase : usize,
    /// 段在文件中的起始偏移。
    pub p_offset : usize,
    /// 段中由文件提供的有效字节数；超过此范围属于零填充。
    pub filesz : usize,
    /// 对应 VMA 的虚拟起点。
    pub vma_start : usize,
    /// VMA 起点对应的文件偏移，用于把缺页地址映射回文件位置。
    pub vma_file_origin : usize,
}


#[path = "cache.rs"]
mod cache;
#[path = "elf.rs"]
mod elf;
#[path = "fault.rs"]
mod fault;
#[path = "mapping.rs"]
mod mapping;
#[path = "vma.rs"]
mod vma;
pub use cache::{load_or_get_readonly_elf_page, load_or_get_readonly_mmap_page,
                test_readonly_elf_page_cache, test_readonly_mmap_page_cache};
pub use elf::*;
pub use fault::*;
pub use mapping::*;
pub use vma::*;

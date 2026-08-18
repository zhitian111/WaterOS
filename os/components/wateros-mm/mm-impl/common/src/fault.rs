//! 双架构共享的惰性 VMA 缺页处理。
//!
//! 本路径只负责判断 VMA、权限和填充策略；成功建立 PTE 后由架构调用方执行适配的 TLB 刷新。

use super::*;

use api_v0::address_space::AddressSpaceOps;
use api_v0::frame_allocator::PhysicalFrameAllocator;
use api_v0::mmap::PageFaultAccess;

/// 架构地址空间类型共享的内部访问器。
///
/// 它让通用惰性文件缺页路径保持泛型，同时不把具体 `LazyVmaSet` 字段暴露到 `api-v0`。
pub trait LazyVmaAccess {
    /// 只读取得当前地址空间的惰性 VMA 集合。
    fn lazy_vma_set(&self) -> &LazyVmaSet;
    fn lazy_vma_set_mut(&mut self) -> &mut LazyVmaSet;
}

/// 通用惰性文件/匿名映射缺页入口。
///
/// VMA 注册表判断故障是否属于惰性映射并提供权限、文件偏移；`VmaBacking` 决定内容策略：匿名页
/// 保持清零，只读文件页可复用页缓存帧，私有/可写页填充到新分配帧。返回 `Ok(true)` 后由调用方
/// 负责 TLB 刷新，因为精确刷新范围取决于架构。
pub fn handle_lazy_file_fault<S, A>(aspace : &mut S,
                                    allocator : &mut A,
                                    fault_addr : VirtAddr,
                                    access : PageFaultAccess)
                                    -> MmResult<bool>
    where S : AddressSpaceOps + LazyVmaAccess,
          A : PhysicalFrameAllocator<FrameId = PhysPageNum>
{
    let page = fault_addr.floor_page()
                         .start_addr();

    let Some(index) = aspace.lazy_vma_set()
                            .lookup(page)
    else {
        return Ok(false);
    };

    let perm = aspace.lazy_vma_set()
                     .get(index)
                     .ok_or(MmError::InvalidAddress)?
                     .perm;
    let allowed = match access {
        PageFaultAccess::Read => perm.readable(),
        PageFaultAccess::Write => perm.writable(),
        PageFaultAccess::Execute => perm.executable(),
    };
    if !allowed || !perm.user() {
        return Ok(false);
    }

    // 另一个 CPU 可能在本 CPU 捕获缺页后已经安装了同页；仍需由调用方刷新本 CPU 的旧 TLB 项。
    if aspace.translate_addr(page)?
             .is_some()
    {
        return Ok(true);
    }

    let file_offset = {
        let vma = aspace.lazy_vma_set()
                        .get(index)
                        .ok_or(MmError::InvalidAddress)?;
        vma.file_offset + (page.0 - vma.start.0)
    };

    if !perm.writable() {
        let backing_page = aspace.lazy_vma_set_mut()
                                 .get_mut(index)
                                 .ok_or(MmError::InvalidAddress)?
                                 .backing
                                 .load_shared_page(file_offset)?;
        if let Some(ppn) = backing_page {
            if let Err(error) = aspace.map_page_to_ppn(page.floor_page(), ppn, perm) {
                let _ = frame_dealloc_result(ppn);
                return Err(error);
            }
            return Ok(true);
        }
    }

    let ppn = alloc_zeroed_frame_with_alloc(allocator)?;
    let pa = phys_access_addr(ppn.0 * PAGE_SIZE);
    let dst = unsafe { core::slice::from_raw_parts_mut(phys_access_addr(pa) as *mut u8, PAGE_SIZE) };

    if let Err(error) = aspace.lazy_vma_set_mut()
                              .get_mut(index)
                              .ok_or(MmError::InvalidAddress)?
                              .backing
                              .load_page(file_offset, dst)
    {
        let _ = allocator.dealloc_frame(ppn);
        return Err(error);
    }

    if let Err(error) = aspace.map_page_to_ppn(page.floor_page(), ppn, perm) {
        let _ = allocator.dealloc_frame(ppn);
        return Err(error);
    }

    Ok(true)
}

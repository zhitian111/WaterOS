//! 双架构共享的 VMA 缺页处理。
//!
//! 本路径只负责判断 VMA、权限和填充策略；成功建立 PTE 后由架构调用方执行适配的 TLB 刷新。

use super::*;

use api_v0::address_space::AddressSpaceOps;
use api_v0::frame_allocator::PhysicalFrameAllocator;
use api_v0::mmap::PageFaultAccess;

/// 架构地址空间类型共享的内部访问器。
///
/// 它让通用 VMA 缺页路径保持泛型，同时不把架构地址空间的内部字段暴露到 `api-v0`。
pub trait VmaAccess {
    /// 只读取得当前地址空间的唯一 VMA 集合。
    fn vma_set(&self) -> &VmaSet;
    /// 取得可修改的 VMA 集合；加载文件页时可能需要更新 loader 的内部状态。
    fn vma_set_mut(&mut self) -> &mut VmaSet;
}

/// 通用惰性文件/匿名映射缺页入口。
///
/// VMA 注册表判断故障是否属于惰性映射并提供权限、文件偏移；`VmaBacking` 决定内容策略：匿名页
/// 保持清零，只读文件页可复用页缓存帧，私有/可写页填充到新分配帧。返回 `Ok(true)` 后由调用方
/// 负责 TLB 刷新，因为精确刷新范围取决于架构。
pub fn handle_vma_fault<S, A>(aspace : &mut S,
                              allocator : &mut A,
                              fault_addr : VirtAddr,
                              access : PageFaultAccess)
                              -> MmResult<bool>
    where S : AddressSpaceOps + VmaAccess,
          A : PhysicalFrameAllocator<FrameId = PhysPageNum>
{
    let page = fault_addr.floor_page()
                         .start_addr();

    let Some(index) = aspace.vma_set()
                            .lookup(page)
    else {
        return Ok(false);
    };

    let vma = aspace.vma_set()
                    .get(index)
                    .ok_or(MmError::InvalidAddress)?;
    if !vma.demand_paged || vma.is_device() {
        return Ok(false);
    }
    let perm = vma.perm;
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
        let vma = aspace.vma_set()
                        .get(index)
                        .ok_or(MmError::InvalidAddress)?;
        vma.file_offset + (page.0 - vma.start.0)
    };

    if !perm.writable() {
        let backing_page = aspace.vma_set_mut()
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
    let pa = ppn.0 * PAGE_SIZE;
    let dst = unsafe { core::slice::from_raw_parts_mut(pa as *mut u8, PAGE_SIZE) };
    // 取得可修改的 VMA；加载文件页时可能需要更新 loader 的内部状态。
    if let Err(error) = aspace.vma_set_mut()
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

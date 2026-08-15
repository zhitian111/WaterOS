use super::*;

use api_v0::address_space::AddressSpaceOps;
use api_v0::frame_allocator::PhysicalFrameAllocator;
use api_v0::mmap::PageFaultAccess;

/// Internal accessor shared by the architecture address-space types.
///
/// It keeps the common lazy-file fault path generic without exposing the
/// concrete `LazyVmaSet` field through `api-v0`.
pub trait LazyVmaAccess {
    fn lazy_vma_set(&self) -> &LazyVmaSet;
    fn lazy_vma_set_mut(&mut self) -> &mut LazyVmaSet;
}

/// Common lazy-file/anonymous fault entry point.
///
/// The VMA registry decides whether the fault belongs to a lazy mapping and
/// supplies the permissions/file offset. `VmaBacking` then owns all content
/// policy: anonymous pages remain zeroed, read-only file pages may return a
/// page-cache frame, and private/writable pages are populated into a freshly
/// allocated frame. The caller is responsible for TLB invalidation after
/// `Ok(true)`, because the exact flush range is architecture-specific.
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

    // A peer may have installed the same page after this CPU took the fault.
    // The caller still needs to flush its stale TLB entry.
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

    let ppn = allocator.alloc_frame()?;
    let pa = ppn.0 * PAGE_SIZE;
    let dst = unsafe { core::slice::from_raw_parts_mut(pa as *mut u8, PAGE_SIZE) };
    dst.fill(0);

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

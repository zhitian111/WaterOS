use super::*;

/// Zero a 4 KiB physical page under the current early-boot direct-access model.
#[inline]
pub fn zero_phys_page(ppn : PhysPageNum) {
    let pa = ppn.0 * PAGE_SIZE;
    unsafe {
        core::ptr::write_bytes(pa as *mut u8, 0, PAGE_SIZE);
    }
}

/// 优先使用 allocator 的预清零能力；不支持时保持原有的 raw 分配后清零语义。
#[inline]
pub fn alloc_zeroed_frame_with_alloc<A>(allocator : &mut A) -> MmResult<PhysPageNum>
    where A : PhysicalFrameAllocator<FrameId = PhysPageNum>
{
    if let Some(frame) = allocator.try_alloc_zeroed_frame()? {
        return Ok(frame);
    }
    let frame = allocator.alloc_frame()?;
    zero_phys_page(frame);
    Ok(frame)
}

/// Computes the page-rounded end address for an mmap request.
pub fn mmap_map_end(base : VirtAddr, len : usize) -> MmResult<VirtAddr> {
    let n_pages = len.checked_add(PAGE_SIZE - 1)
                     .ok_or(MmError::InvalidAddress)? /
                  PAGE_SIZE;
    Ok(VirtAddr(base.0
                    .checked_add(n_pages * PAGE_SIZE)
                    .ok_or(MmError::InvalidAddress)?))
}

const MAX_MMAP_SEARCH_PAGES : usize = 1 << 20;

/// Finds the first fully unmapped page range large enough for an mmap request.
pub fn find_free_mmap_base<S>(aspace : &S, cursor : VirtAddr, len : usize) -> MmResult<VirtAddr>
    where S : AddressSpaceOps {
    if len == 0 {
        return Err(MmError::InvalidAddress);
    }
    let n_pages = len.checked_add(PAGE_SIZE - 1)
                     .ok_or(MmError::InvalidAddress)? /
                  PAGE_SIZE;
    let mut base = cursor.ceil_page()
                         .start_addr();
    let mut skipped = 0usize;
    loop {
        if skipped > MAX_MMAP_SEARCH_PAGES {
            return Err(MmError::InvalidAddress);
        }
        let mut free = true;
        for i in 0..n_pages {
            let va = VirtAddr(base.0
                                  .checked_add(i.checked_mul(PAGE_SIZE)
                                                .ok_or(MmError::InvalidAddress)?)
                                  .ok_or(MmError::InvalidAddress)?);
            if aspace.translate_addr(va)?
                     .is_some()
            {
                free = false;
                break;
            }
        }
        if free {
            return Ok(base);
        }
        skipped += 1;
        base = VirtAddr(base.0
                            .checked_add(PAGE_SIZE)
                            .ok_or(MmError::InvalidAddress)?);
    }
}

/// Maps `[start, end)` to freshly allocated zeroed frames.
pub fn map_zeroed_range_with_alloc<S, A>(aspace : &mut S,
                                         allocator : &mut A,
                                         start : VirtAddr,
                                         end : VirtAddr,
                                         perm : PagePerm)
                                         -> MmResult<()>
    where S : AddressSpaceOps,
          A : PhysicalFrameAllocator<FrameId = PhysPageNum>
{
    if start.0 >= end.0 {
        return Ok(());
    }
    let mut vpn = start.floor_page();
    let vpn_end = end.ceil_page();
    while vpn.0 < vpn_end.0 {
        map_zeroed_page_with_alloc(aspace, allocator, vpn, perm)?;
        vpn = VirtPageNum(vpn.0 + 1);
    }
    Ok(())
}

/// Maps one virtual page to a freshly allocated zeroed frame.
pub fn map_zeroed_page_with_alloc<S, A>(aspace : &mut S,
                                        allocator : &mut A,
                                        vpn : VirtPageNum,
                                        perm : PagePerm)
                                        -> MmResult<()>
    where S : AddressSpaceOps,
          A : PhysicalFrameAllocator<FrameId = PhysPageNum>
{
    let ppn = alloc_zeroed_frame_with_alloc(allocator)?;
    aspace.map_page_to_ppn(vpn, ppn, perm)
}

/// Maps `[base, end)` to freshly allocated frames filled from `backing`.
pub fn map_range_from_backing<S, A>(aspace : &mut S,
                                    allocator : &mut A,
                                    base : VirtAddr,
                                    end : VirtAddr,
                                    perm : PagePerm,
                                    backing : &[u8])
                                    -> MmResult<()>
    where S : AddressSpaceOps,
          A : PhysicalFrameAllocator<FrameId = PhysPageNum>
{
    let mut vpn = base.floor_page();
    let vpn_end = end.ceil_page();
    let mut page_index = 0usize;
    while vpn.0 < vpn_end.0 {
        let ppn = allocator.alloc_frame()?;
        fill_phys_page(ppn, page_index, backing);
        aspace.map_page_to_ppn(vpn, ppn, perm)?;
        vpn = VirtPageNum(vpn.0 + 1);
        page_index += 1;
    }
    Ok(())
}

/// Maps `[base, end)` to freshly allocated frames filled by `load_page`.
pub fn map_range_from_loader<S, A, F>(aspace : &mut S,
                                      allocator : &mut A,
                                      base : VirtAddr,
                                      end : VirtAddr,
                                      perm : PagePerm,
                                      mut load_page : F)
                                      -> MmResult<()>
    where S : AddressSpaceOps,
          A : PhysicalFrameAllocator<FrameId = PhysPageNum>,
          F : FnMut(usize, &mut [u8]) -> MmResult<()>
{
    let mut vpn = base.floor_page();
    let vpn_end = end.ceil_page();
    let mut page_index = 0usize;
    while vpn.0 < vpn_end.0 {
        let ppn = alloc_zeroed_frame_with_alloc(allocator)?;
        let pa = ppn.0 * PAGE_SIZE;
        let page = unsafe { core::slice::from_raw_parts_mut(pa as *mut u8, PAGE_SIZE) };
        if let Err(e) = load_page(page_index, page) {
            let _ = allocator.dealloc_frame(ppn);
            let _ = aspace.unmap_range_with_alloc(allocator, base, vpn.start_addr());
            return Err(e);
        }
        if let Err(e) = aspace.map_page_to_ppn(vpn, ppn, perm) {
            let _ = allocator.dealloc_frame(ppn);
            let _ = aspace.unmap_range_with_alloc(allocator, base, vpn.start_addr());
            return Err(e);
        }
        vpn = VirtPageNum(vpn.0 + 1);
        page_index += 1;
    }
    Ok(())
}

/// Fills one physical page with the corresponding chunk from `src`.
pub fn fill_phys_page(ppn : PhysPageNum, page_index : usize, src : &[u8]) {
    let pa = ppn.0 * PAGE_SIZE;
    let page = unsafe { core::slice::from_raw_parts_mut(pa as *mut u8, PAGE_SIZE) };
    page.fill(0);
    let start = page_index * PAGE_SIZE;
    if start >= src.len() {
        return;
    }
    let end = (start + PAGE_SIZE).min(src.len());
    page[..end - start].copy_from_slice(&src[start..end]);
}

/// Linux `MREMAP_*` flags understood by [`mremap_range`].
pub const MREMAP_MAYMOVE : usize = 1;
pub const MREMAP_FIXED : usize = 2;
pub const MREMAP_DONTUNMAP : usize = 4;
const MREMAP_KNOWN : usize = MREMAP_MAYMOVE | MREMAP_FIXED | MREMAP_DONTUNMAP;

fn region_is_mapped<S : AddressSpaceOps>(aspace : &S,
                                         start : VirtAddr,
                                         end_exclusive : VirtAddr)
                                         -> MmResult<bool> {
    let mut vpn = start.floor_page();
    let vpn_end = end_exclusive.ceil_page();
    while vpn.0 < vpn_end.0 {
        if aspace.translate_addr(vpn.start_addr())?
                 .is_none()
        {
            return Ok(false);
        }
        vpn = VirtPageNum(vpn.0 + 1);
    }
    Ok(true)
}

fn copy_mapped_bytes<S : AddressSpaceOps>(aspace : &S,
                                          src : VirtAddr,
                                          dst : VirtAddr,
                                          len : usize)
                                          -> MmResult<()> {
    let mut offset = 0usize;
    while offset < len {
        let src_va = VirtAddr(src.0
                                 .checked_add(offset)
                                 .ok_or(MmError::InvalidAddress)?);
        let dst_va = VirtAddr(dst.0
                                 .checked_add(offset)
                                 .ok_or(MmError::InvalidAddress)?);
        let src_pa = aspace.translate_addr(src_va)?
                           .ok_or(MmError::NotMapped)?;
        let dst_pa = aspace.translate_addr(dst_va)?
                           .ok_or(MmError::NotMapped)?;
        let page_off = offset % PAGE_SIZE;
        let chunk = core::cmp::min(PAGE_SIZE - page_off, len - offset);
        unsafe {
            core::ptr::copy_nonoverlapping((src_pa.0 + page_off) as *const u8,
                                           (dst_pa.0 + page_off) as *mut u8,
                                           chunk);
        }
        offset += chunk;
    }
    Ok(())
}

fn mremap_relocate<S, A>(aspace : &mut S,
                         allocator : &mut A,
                         old_start : VirtAddr,
                         old_end : VirtAddr,
                         new_size : usize,
                         new_base : VirtAddr,
                         perm : PagePerm,
                         unmap_old : bool)
                         -> MmResult<VirtAddr>
    where S : AddressSpaceOps,
          A : PhysicalFrameAllocator<FrameId = PhysPageNum>
{
    let new_end = mmap_map_end(new_base, new_size)?;
    map_zeroed_range_with_alloc(aspace,
                                allocator,
                                new_base,
                                new_end,
                                perm)?;
    let copy_len = core::cmp::min(old_end.0
                                         .saturating_sub(old_start.0),
                                  new_end.0
                                         .saturating_sub(new_base.0));
    copy_mapped_bytes(aspace, old_start, new_base, copy_len)?;
    if unmap_old {
        aspace.unmap_range_with_alloc(allocator, old_start, old_end)?;
    }
    Ok(new_base)
}

/// Linux `mremap(2)` 语义子集：匿名私有映射 grow/shrink、`MREMAP_MAYMOVE` 搬迁。
pub fn mremap_range<S, A>(aspace : &mut S,
                          allocator : &mut A,
                          old_addr : VirtAddr,
                          old_size : usize,
                          new_size : usize,
                          flags : usize,
                          new_address : VirtAddr,
                          relocation_base : VirtAddr,
                          force_move : bool,
                          perm : PagePerm)
                          -> MmResult<VirtAddr>
    where S : AddressSpaceOps,
          A : PhysicalFrameAllocator<FrameId = PhysPageNum>
{
    if new_size == 0 {
        return Err(MmError::InvalidAddress);
    }
    if flags & !MREMAP_KNOWN != 0 {
        return Err(MmError::InvalidAddress);
    }
    if flags & MREMAP_FIXED != 0 && flags & MREMAP_MAYMOVE == 0 {
        return Err(MmError::InvalidAddress);
    }
    if flags & MREMAP_DONTUNMAP != 0 && flags & MREMAP_FIXED == 0 {
        return Err(MmError::InvalidAddress);
    }

    let old_start = old_addr.floor_page()
                            .start_addr();
    let old_end = VirtAddr(old_addr.0
                                   .checked_add(old_size)
                                   .ok_or(MmError::InvalidAddress)?).ceil_page()
                                                                    .start_addr();
    let new_end = VirtAddr(old_addr.0
                                   .checked_add(new_size)
                                   .ok_or(MmError::InvalidAddress)?).ceil_page()
                                                                    .start_addr();

    if !region_is_mapped(aspace, old_start, old_end)? {
        return Err(MmError::NotMapped);
    }

    if flags & MREMAP_FIXED != 0 {
        // 固定地址搬迁：先清空目标区间，再拷贝旧内容
        if new_address.0 % PAGE_SIZE != 0 {
            return Err(MmError::InvalidAddress);
        }
        let dest_start = new_address.floor_page()
                                    .start_addr();
        let dest_end = VirtAddr(new_address.0
                                           .checked_add(new_size)
                                           .ok_or(MmError::InvalidAddress)?).ceil_page()
                                                                            .start_addr();
        if dest_start.0 < old_end.0 && dest_end.0 > old_start.0 {
            return Err(MmError::InvalidAddress);
        }
        aspace.unmap_range_with_alloc(allocator, dest_start, dest_end)?;
        map_zeroed_range_with_alloc(aspace,
                                    allocator,
                                    dest_start,
                                    dest_end,
                                    perm)?;
        let copy_len = core::cmp::min(old_end.0
                                             .saturating_sub(old_start.0),
                                      dest_end.0
                                              .saturating_sub(dest_start.0));
        copy_mapped_bytes(aspace, old_start, dest_start, copy_len)?;
        if flags & MREMAP_DONTUNMAP == 0 {
            aspace.unmap_range_with_alloc(allocator, old_start, old_end)?;
        }
        return Ok(new_address);
    }

    if new_end.0 <= old_end.0 {
        // 缩小或等长：截断尾部映射即可
        if new_end.0 < old_end.0 {
            aspace.unmap_range_with_alloc(allocator, new_end, old_end)?;
        }
        return Ok(old_addr);
    }

    if force_move {
        if flags & MREMAP_MAYMOVE == 0 {
            return Err(MmError::InvalidAddress);
        }
        return mremap_relocate(aspace,
                               allocator,
                               old_start,
                               old_end,
                               new_size,
                               relocation_base,
                               perm,
                               true);
    }

    let mut vpn = old_end.floor_page();
    let grow_end = new_end.ceil_page();
    while vpn.0 < grow_end.0 {
        if aspace.translate_addr(vpn.start_addr())?
                 .is_some()
        {
            // 原位增长会与已有映射冲突，需 MAYMOVE 整体搬迁
            if flags & MREMAP_MAYMOVE == 0 {
                return Err(MmError::InvalidAddress);
            }
            return mremap_relocate(aspace,
                                   allocator,
                                   old_start,
                                   old_end,
                                   new_size,
                                   relocation_base,
                                   perm,
                                   true);
        }
        vpn = VirtPageNum(vpn.0 + 1);
    }

    map_zeroed_range_with_alloc(aspace,
                                allocator,
                                old_end,
                                new_end,
                                perm)?;
    Ok(old_addr)
}

//! Shared helpers for concrete MM implementations.
//!
//! This crate intentionally contains implementation helpers rather than public
//! MM contracts. It stays below `wateros-mm-api-v0`: helpers here may depend on
//! current loader policy, while `mm-api` remains the stable semantic boundary.

#![no_std]

extern crate alloc;

use alloc::vec::Vec;

use api_v0::addr::{PhysPageNum, VirtAddr, VirtPageNum, PAGE_SIZE};
use api_v0::address_space::AddressSpaceOps;
use api_v0::error::{MmError, MmResult};
use api_v0::executable;
use api_v0::frame_allocator::PhysicalFrameAllocator;
use api_v0::kernel_bringup::LoadElfError;
use api_v0::perm::PagePerm;

/// ELF program header type for loadable segments.
pub const PT_LOAD : u32 = 1;

/// Little-endian `u16` read; returns `None` on out-of-bounds input.
#[inline]
pub fn rd_u16(s : &[u8], o : usize) -> Option<u16> {
    s.get(o..o + 2)?
     .try_into()
     .ok()
     .map(u16::from_le_bytes)
}

/// Little-endian `u32` read; returns `None` on out-of-bounds input.
#[inline]
pub fn rd_u32(s : &[u8], o : usize) -> Option<u32> {
    s.get(o..o + 4)?
     .try_into()
     .ok()
     .map(u32::from_le_bytes)
}

/// Little-endian `u64` read; returns `None` on out-of-bounds input.
#[inline]
pub fn rd_u64(s : &[u8], o : usize) -> Option<u64> {
    s.get(o..o + 8)?
     .try_into()
     .ok()
     .map(u64::from_le_bytes)
}

/// Checks only the ELF64 little-endian prefix accepted by `mm-api`.
#[inline]
pub fn elf64_le_prefix_ok(data : &[u8]) -> bool { executable::is_elf_prefix(data) }

/// Text/script inputs should not trigger ELF read retries.
#[inline]
pub fn skip_elf_prefix_retry(data : &[u8]) -> bool { executable::is_text_file(data) }

/// Checks that `e_entry` is inside a loadable segment.
///
/// This catches images whose ELF prefix looks fine but whose program headers or
/// entry point were read inconsistently from the backing filesystem.
pub fn elf_entry_plausible(data : &[u8]) -> bool {
    if data.len() < 0x40 {
        return false;
    }
    let e_entry = match rd_u64(data, 0x18) {
        Some(v) => v as usize,
        None => return false,
    };
    if e_entry == 0 {
        return false;
    }
    let e_phoff = match rd_u64(data, 0x20) {
        Some(v) => v as usize,
        None => return false,
    };
    let e_phentsize = match rd_u16(data, 0x36) {
        Some(v) => v as usize,
        None => return false,
    };
    let e_phnum = match rd_u16(data, 0x38) {
        Some(v) => v as usize,
        None => return false,
    };
    if e_phentsize < 56 || e_phnum == 0 {
        return false;
    }
    for i in 0..e_phnum {
        let ph = e_phoff + i * e_phentsize;
        if ph + 56 > data.len() {
            return false;
        }
        if rd_u32(data, ph) != Some(PT_LOAD) {
            continue;
        }
        let p_vaddr = match rd_u64(data, ph + 16) {
            Some(v) => v as usize,
            None => return false,
        };
        let p_memsz = match rd_u64(data, ph + 40) {
            Some(v) => v as usize,
            None => return false,
        };
        if p_memsz == 0 {
            continue;
        }
        let Some(p_end) = p_vaddr.checked_add(p_memsz) else {
            return false;
        };
        if e_entry >= p_vaddr && e_entry < p_end {
            return true;
        }
    }
    false
}

/// Returns whether an ELF read is acceptable for loading.
#[inline]
pub fn elf_read_acceptable(data : &[u8]) -> bool {
    elf64_le_prefix_ok(data) && elf_entry_plausible(data)
}

/// Stabilizes reads of ELF bytes from a root filesystem.
///
/// If two reads disagree, a third read is used as a tiebreaker; otherwise the
/// first acceptable image is selected. Non-ELF text files are returned as-is so
/// script/shebang probing does not produce noisy retries.
pub fn finalize_elf_read(path : &str,
                         first : Vec<u8>,
                         read_again : impl Fn() -> Result<Vec<u8>, LoadElfError>)
                         -> Result<Vec<u8>, LoadElfError> {
    if skip_elf_prefix_retry(&first) || !elf64_le_prefix_ok(&first) {
        return Ok(first);
    }
    let second = read_again()?;
    if first == second {
        if elf_read_acceptable(&first) {
            return Ok(first);
        }
        if !elf_read_acceptable(&second) {
            let n = second.len().min(16);
            runtime::logging::warn!("[elf-load] stable read bad ELF64-LE image (len={} \
                                     first{}={:02x?}) path={}",
                                    second.len(),
                                    n,
                                    &second[..n],
                                    path);
        }
        return Ok(second);
    }
    runtime::logging::warn!("[elf-load] inconsistent ELF reads path={} len {} vs {}; third read",
                            path,
                            first.len(),
                            second.len());
    let third = read_again()?;
    if second == third && elf_read_acceptable(&second) {
        return Ok(second);
    }
    if first == third && elf_read_acceptable(&first) {
        return Ok(first);
    }
    if elf_read_acceptable(&second) {
        return Ok(second);
    }
    if elf_read_acceptable(&third) {
        return Ok(third);
    }
    if elf_read_acceptable(&first) {
        return Ok(first);
    }
    Ok(second)
}

/// Finds the file offset backing an entry PC inside a `PT_LOAD` segment.
pub fn entry_file_offset(data : &[u8], entry_pc : usize) -> Option<usize> {
    let e_phoff = rd_u64(data, 0x20)? as usize;
    let e_phentsize = rd_u16(data, 0x36)? as usize;
    let e_phnum = rd_u16(data, 0x38)? as usize;
    if e_phentsize < 56 || e_phnum == 0 {
        return None;
    }
    for i in 0..e_phnum {
        let ph = e_phoff + i * e_phentsize;
        if ph + 56 > data.len() {
            return None;
        }
        if rd_u32(data, ph)? != PT_LOAD {
            continue;
        }
        let p_vaddr = rd_u64(data, ph + 16)? as usize;
        let p_offset = rd_u64(data, ph + 8)? as usize;
        let p_memsz = rd_u64(data, ph + 40)? as usize;
        let p_end = p_vaddr.checked_add(p_memsz)?;
        if entry_pc >= p_vaddr && entry_pc < p_end {
            return p_offset.checked_add(entry_pc - p_vaddr);
        }
    }
    None
}

/// Zero a 4 KiB physical page under the current early-boot direct-access model.
#[inline]
pub fn zero_phys_page(ppn : PhysPageNum) {
    let pa = ppn.0 * PAGE_SIZE;
    unsafe {
        core::ptr::write_bytes(pa as *mut u8, 0, PAGE_SIZE);
    }
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
    let ppn = allocator.alloc_frame()?;
    zero_phys_page(ppn);
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
        let ppn = allocator.alloc_frame()?;
        let pa = ppn.0 * PAGE_SIZE;
        let page = unsafe { core::slice::from_raw_parts_mut(pa as *mut u8, PAGE_SIZE) };
        page.fill(0);
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

const MREMAP_STACK_PERM : PagePerm = PagePerm(PagePerm::R.0 | PagePerm::W.0 | PagePerm::U.0);

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
                         search_cursor : VirtAddr,
                         unmap_old : bool)
                         -> MmResult<VirtAddr>
    where S : AddressSpaceOps,
          A : PhysicalFrameAllocator<FrameId = PhysPageNum>
{
    let new_base = find_free_mmap_base(aspace, search_cursor, new_size)?;
    let new_end = mmap_map_end(new_base, new_size)?;
    map_zeroed_range_with_alloc(aspace,
                                allocator,
                                new_base,
                                new_end,
                                MREMAP_STACK_PERM)?;
    let copy_len = old_end.0
                          .saturating_sub(old_start.0);
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
                          search_cursor : VirtAddr)
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
        if new_address.0 % PAGE_SIZE != 0 {
            return Err(MmError::InvalidAddress);
        }
        let dest_start = new_address.floor_page()
                                    .start_addr();
        let dest_end = VirtAddr(new_address.0
                                           .checked_add(new_size)
                                           .ok_or(MmError::InvalidAddress)?).ceil_page()
                                                                            .start_addr();
        aspace.unmap_range_with_alloc(allocator, dest_start, dest_end)?;
        map_zeroed_range_with_alloc(aspace,
                                    allocator,
                                    dest_start,
                                    dest_end,
                                    MREMAP_STACK_PERM)?;
        let copy_len = old_end.0
                              .saturating_sub(old_start.0);
        copy_mapped_bytes(aspace, old_start, dest_start, copy_len)?;
        if flags & MREMAP_DONTUNMAP == 0 {
            aspace.unmap_range_with_alloc(allocator, old_start, old_end)?;
        }
        return Ok(new_address);
    }

    if new_end.0 <= old_end.0 {
        if new_end.0 < old_end.0 {
            aspace.unmap_range_with_alloc(allocator, new_end, old_end)?;
        }
        return Ok(old_addr);
    }

    let mut vpn = old_end.floor_page();
    let grow_end = new_end.ceil_page();
    while vpn.0 < grow_end.0 {
        if aspace.translate_addr(vpn.start_addr())?
                 .is_some()
        {
            if flags & MREMAP_MAYMOVE == 0 {
                return Err(MmError::InvalidAddress);
            }
            return mremap_relocate(aspace,
                                   allocator,
                                   old_start,
                                   old_end,
                                   new_size,
                                   search_cursor,
                                   true);
        }
        vpn = VirtPageNum(vpn.0 + 1);
    }

    map_zeroed_range_with_alloc(aspace,
                                allocator,
                                old_end,
                                new_end,
                                MREMAP_STACK_PERM)?;
    Ok(old_addr)
}

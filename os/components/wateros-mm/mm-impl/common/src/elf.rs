use super::*;

impl ElfSegmentLoadParams {
    pub fn page_va_from_file_offset(&self, file_offset : usize) -> usize {
        self.vma_start + file_offset.saturating_sub(self.vma_file_origin)
    }

    pub fn fill_page<F>(&self,
                        file_offset : usize,
                        dst : &mut [u8],
                        read_file : F)
                        -> MmResult<()>
        where F : FnMut(usize, &mut [u8]) -> MmResult<()>
    {
        fill_elf_load_page(self.vbase,
                           self.p_offset,
                           self.filesz,
                           self.page_va_from_file_offset(file_offset),
                           dst,
                           read_file)
    }
}

/// `PT_LOAD` 程序头类型（可装载段）。
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


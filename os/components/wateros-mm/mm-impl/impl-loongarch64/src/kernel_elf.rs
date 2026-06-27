//! 从根文件系统装载 LoongArch ELF64（小端），建立独立用户地址空间并映射
//! `PT_LOAD` 与用户栈（分页格式由 mm-impl 完成，当前为 LoongArch64 三级页表）。
//!
//! 用户地址空间内 **额外** 恒等映射内核 RAM（与 [`crate::kernel_global`] 的
//! `phys_ram_end_exclusive`
//! 一致），便于同一套页表里内核辅助访问；用户段不得与用户 VPN 或 `0x9000_0000`
//! 以上恒等区非法重叠（重叠时返回 `Parse`）。

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use core::cmp;

use api_v0::addr::{VirtAddr, VirtPageNum, PAGE_SIZE};
use api_v0::address_space::AddressSpaceOps;
use api_v0::error::MmError;
use api_v0::kernel_bringup::{LoadElfError, LoadedElf, RootVolumeReadError};
use api_v0::perm::PagePerm;
use frame_alloctor::frame_alloc_result;
#[cfg(not(feature = "vfs-root-read"))]
use fs::api::{FsError, SharedFs};
use impl_common::{entry_file_offset, finalize_elf_read, rd_u16, rd_u32, rd_u64, PT_LOAD};

use crate::pagetable::{zero_phys_page, LoongArch64AddressSpace};

#[cfg(feature = "vfs-root-read")]
use vfs::api::{SingleRootReadView, VfsError};

#[cfg(not(feature = "vfs-root-read"))]
#[inline]
fn map_fs_to_root_vol(e : FsError) -> RootVolumeReadError {
    match e {
        FsError::NotMounted => RootVolumeReadError::NotMounted,
        FsError::NotFound => RootVolumeReadError::NotFound,
        FsError::NotAFile => RootVolumeReadError::NotAFile,
        FsError::InvalidPath => RootVolumeReadError::InvalidPath,
        FsError::NotUtf8 => RootVolumeReadError::NotUtf8,
        FsError::Unsupported => RootVolumeReadError::Unsupported,
        FsError::Driver => RootVolumeReadError::Driver,
        FsError::Corrupt => RootVolumeReadError::Corrupt,
        FsError::Io => RootVolumeReadError::Io,
        FsError::Exists => RootVolumeReadError::Unsupported,
    }
}

#[cfg(feature = "vfs-root-read")]
#[inline]
fn map_vfs_to_root_vol(e : VfsError) -> RootVolumeReadError {
    match e {
        VfsError::NotMounted => RootVolumeReadError::NotMounted,
        VfsError::NotFound => RootVolumeReadError::NotFound,
        VfsError::NotAFile => RootVolumeReadError::NotAFile,
        VfsError::InvalidPath | VfsError::Exists => RootVolumeReadError::InvalidPath,
        VfsError::NotUtf8 => RootVolumeReadError::NotUtf8,
        VfsError::Unsupported => RootVolumeReadError::Unsupported,
        VfsError::Driver => RootVolumeReadError::Driver,
        VfsError::Corrupt => RootVolumeReadError::Corrupt,
        VfsError::Io => RootVolumeReadError::Io,
        VfsError::BadFd |
        VfsError::Busy |
        VfsError::WouldBlock |
        VfsError::Interrupted |
        VfsError::BrokenPipe |
        VfsError::NoTask |
        VfsError::TooManyOpenFiles |
        VfsError::NoSpace |
        VfsError::ReadOnlyFs => RootVolumeReadError::Unsupported,
        VfsError::Busy => RootVolumeReadError::Io,
    }
}

const EM_LOONGARCH : u16 = 258;
const PT_INTERP : u32 = 3;
const LOONGARCH64_USER_STACK_TOP : usize = 0x0000_007F_FFFF_A000;
const LOONGARCH64_INTERP_BASE : usize = 0x0000_0000_7000_0000;
const USER_STACK_SIZE : usize = 256 * 1024;
const USER_STACK_PREMAP_PAGES : usize = 16;
const PREFERRED_MMAP_BASE : usize = 0x1000_0000;
const USER_HEAP_MMAP_GAP : usize = 64 * 1024 * 1024;
const MUSL_LIBC_PATH : &str = "/musl/lib/libc.so";
const LOONGARCH_INSN_SYSCALL : u32 = 0x002b_0000;
const LOONGARCH_INSN_RET : u32 = 0x4c00_0020;
const LOONGARCH_INSN_SLLI_W_A0_A0_0 : u32 = 0x0040_8084;
const LOONGARCH_MUSL_SCHED_STUB_MARKER : u32 = 0x02bf_6804; // li.w a0, -ENOSYS

struct MuslSchedStubPatch {
    offset : usize,
    syscall_nr : u32,
    name : &'static str,
}

const MUSL_SCHED_STUB_PATCHES : &[MuslSchedStubPatch] =
    &[MuslSchedStubPatch { offset : 0x54544,
                           syscall_nr : 118,
                           name : "sched_setparam" },
      MuslSchedStubPatch { offset : 0x54564,
                           syscall_nr : 119,
                           name : "sched_setscheduler" },
      MuslSchedStubPatch { offset : 0x54500,
                           syscall_nr : 120,
                           name : "sched_getscheduler" },
      MuslSchedStubPatch { offset : 0x544e0,
                           syscall_nr : 121,
                           name : "sched_getparam" }];

struct ElfHeaderInfo {
    entry : usize,
    phentsize : usize,
    phnum : usize,
    phdrs : Vec<u8>,
}

#[inline]
fn initial_mmap_base(heap_start : VirtAddr) -> VirtAddr {
    VirtAddr(cmp::max(heap_start.0
                                .saturating_add(USER_HEAP_MMAP_GAP),
                      PREFERRED_MMAP_BASE))
}

/// 从根 RO 句柄读整文件；ELF 路径双读校验（见 common `finalize_elf_read`）。
#[cfg(not(feature = "vfs-root-read"))]
fn read_whole_file_ro_retry_bad_prefix(root : &SharedFs,
                                       path : &str)
                                       -> Result<Vec<u8>, LoadElfError> {
    let read_once = || {
        let g = root.lock();
        g.read(path)
         .map_err(|e| {
             runtime::logging::trace!("[elf-load] abort: Fs::read err={:?} path={}",
                                      e,
                                      path);
             LoadElfError::RootVolume(map_fs_to_root_vol(e))
         })
    };
    let first = read_once()?;
    finalize_elf_read(path, first, read_once)
}

/// 从根卷读取 `path` 的完整字节（含 ELF 双读校验）。
pub fn read_path_bytes(path : &str) -> Result<Vec<u8>, LoadElfError> {
    #[cfg(feature = "vfs-root-read")]
    {
        let view = vfs::root::read_view();
        read_whole_file_ro_retry_bad_prefix_vfs(view, path)
    }
    #[cfg(not(feature = "vfs-root-read"))]
    {
        let root = fs::rootfs::active_impl::root_fs().ok_or_else(|| {
                       runtime::logging::trace!("[elf-load] abort: no root_fs (mount/driver?)");
                       LoadElfError::NoRootFs
                   })?;
        read_whole_file_ro_retry_bad_prefix(&root, path)
    }
}

fn read_path_range(path : &str, offset : u64, buf : &mut [u8]) -> Result<usize, LoadElfError> {
    #[cfg(feature = "vfs-root-read")]
    {
        let view = vfs::root::read_view();
        view.read_range(path, offset, buf)
            .map_err(|e| {
                runtime::logging::trace!("[elf-load] abort: Vfs::read_range err={:?} path={} \
                                          offset={} len={}",
                                         e,
                                         path,
                                         offset,
                                         buf.len());
                LoadElfError::RootVolume(map_vfs_to_root_vol(e))
            })
    }
    #[cfg(not(feature = "vfs-root-read"))]
    {
        let root = fs::rootfs::active_impl::root_fs().ok_or_else(|| {
                       runtime::logging::trace!("[elf-load] abort: no root_fs (mount/driver?)");
                       LoadElfError::NoRootFs
                   })?;
        let g = root.lock();
        g.read_range(path, offset, buf)
         .map_err(|e| {
             runtime::logging::trace!("[elf-load] abort: Fs::read_range err={:?} path={} \
                                       offset={} len={}",
                                      e,
                                      path,
                                      offset,
                                      buf.len());
             LoadElfError::RootVolume(map_fs_to_root_vol(e))
         })
    }
}

fn read_path_exact(path : &str, offset : u64, buf : &mut [u8]) -> Result<(), LoadElfError> {
    let mut filled = 0usize;
    while filled < buf.len() {
        let n = read_path_range(path,
                                offset + filled as u64,
                                &mut buf[filled..])?;
        if n == 0 {
            return Err(LoadElfError::Parse);
        }
        filled += n;
    }
    Ok(())
}

fn read_elf_header_info(path : &str) -> Result<ElfHeaderInfo, LoadElfError> {
    let mut ehdr = [0u8; 64];
    read_path_exact(path, 0, &mut ehdr)?;
    if &ehdr[0..4] != b"\x7FELF" {
        return Err(LoadElfError::BadMagic);
    }
    if ehdr.get(4) != Some(&2) {
        return Err(LoadElfError::BadClass);
    }
    if ehdr.get(5) != Some(&1) {
        return Err(LoadElfError::BadEndian);
    }
    if rd_u16(&ehdr, 18).ok_or(LoadElfError::TooSmall)? != EM_LOONGARCH {
        return Err(LoadElfError::BadMachine);
    }
    let entry = rd_u64(&ehdr, 0x18).ok_or(LoadElfError::TooSmall)? as usize;
    let phoff = rd_u64(&ehdr, 0x20).ok_or(LoadElfError::TooSmall)? as usize;
    let phentsize = rd_u16(&ehdr, 0x36).ok_or(LoadElfError::TooSmall)? as usize;
    let phnum = rd_u16(&ehdr, 0x38).ok_or(LoadElfError::TooSmall)? as usize;
    if phentsize < 56 || phnum == 0 {
        return Err(LoadElfError::Parse);
    }

    let phdr_len = phentsize.checked_mul(phnum)
                            .ok_or(LoadElfError::Parse)?;
    let mut phdrs = Vec::new();
    phdrs.resize(phdr_len, 0);
    read_path_exact(path, phoff as u64, &mut phdrs)?;
    Ok(ElfHeaderInfo { entry,
                       phentsize,
                       phnum,
                       phdrs })
}

/// 从根卷读取 `path` 的前缀，最多 `len` 字节；用于 exec 探测，避免大 ELF 整读。
pub fn read_path_prefix(path : &str, len : usize) -> Result<Vec<u8>, LoadElfError> {
    let mut data = Vec::new();
    data.resize(len, 0);
    let n = read_path_range(path, 0, &mut data)?;
    data.truncate(n);
    Ok(data)
}

#[cfg(feature = "vfs-root-read")]
fn read_whole_file_ro_retry_bad_prefix_vfs(view : &dyn SingleRootReadView,
                                           path : &str)
                                           -> Result<Vec<u8>, LoadElfError> {
    let read_once = || {
        view.read(path)
            .map_err(|e| {
                runtime::logging::trace!("[elf-load] abort: Vfs::read err={:?} path={}",
                                         e,
                                         path);
                LoadElfError::RootVolume(map_vfs_to_root_vol(e))
            })
    };
    let first = read_once()?;
    finalize_elf_read(path, first, read_once)
}

// ELF `p_flags`：bit2=R，bit1=W，bit0=X；全无时补 `R`。
fn perm_from_pf(p_flags : u32) -> PagePerm {
    let mut p = PagePerm::U;
    if p_flags & 4 != 0 {
        p |= PagePerm::R;
    }
    if p_flags & 2 != 0 {
        p |= PagePerm::W;
    }
    if p_flags & 1 != 0 {
        p |= PagePerm::X;
    }
    if p == PagePerm::U {
        p |= PagePerm::R;
    }
    p
}

/// 将 `[0x9000_0000, phys_ram_end)` 以 `vpn==ppn` 恒等映射进用户页表，权限
/// `R|W|X`（内核辅助访问与用户段装载共用一套表时的 bring-up 约定）。
fn map_kernel_ram_identity<A : AddressSpaceOps>(aspace : &mut A) -> Result<(), LoadElfError> {
    let lo = VirtAddr(0x9000_0000).floor_page();
    let hi = VirtAddr(crate::kernel_global::phys_ram_end_exclusive()).ceil_page();
    for vpn_raw in lo.0..hi.0 {
        let vpn = VirtPageNum(vpn_raw);
        let ppn = vpn.to_phys_page_identity();
        aspace.map_page_to_ppn(vpn,
                               ppn,
                               PagePerm::R | PagePerm::W | PagePerm::X)
              .map_err(LoadElfError::Mm)?;
    }
    Ok(())
}

/// 为单个 `PT_LOAD` 分配/合并映射并填充内容。
fn map_segment<A : AddressSpaceOps>(aspace : &mut A,
                                    file : &[u8],
                                    p_vaddr : u64,
                                    p_offset : u64,
                                    p_filesz : u64,
                                    p_memsz : u64,
                                    perm : PagePerm)
                                    -> Result<(), LoadElfError> {
    let vbase = p_vaddr as usize;
    let memsz = p_memsz as usize;
    let filesz = p_filesz as usize;
    if memsz == 0 {
        return Ok(());
    }
    let fo = p_offset as usize;
    let va_start = VirtAddr(vbase);
    let va_end = VirtAddr(vbase + memsz);
    let mut vpn = va_start.floor_page();
    let vpn_end = va_end.ceil_page();
    while vpn.0 < vpn_end.0 {
        if let Some(_pa) = aspace.translate_addr(vpn.start_addr())
                                 .map_err(LoadElfError::Mm)?
        {
            // 与上一段 PT_LOAD 共享页：合并权限，勿再分配帧。
            if vpn.start_addr().0 >= 0x9000_0000 {
                runtime::logging::trace!("[elf-load] PT_LOAD refuse overlap with kernel identity \
                                          VPN={:#x}",
                                         vpn.0);
                return Err(LoadElfError::Parse);
            }
            let old = aspace.leaf_page_perm(vpn)
                            .map_err(LoadElfError::Mm)?
                            .unwrap_or(PagePerm::empty());
            let merged = old | perm;
            aspace.protect_page(vpn, merged)
                  .map_err(LoadElfError::Mm)?;
            runtime::logging::trace!("[elf-load] PT_LOAD shared VPN={:#x} merge perm {:?}|{:?} \
                                      -> {:?}",
                                     vpn.0,
                                     old,
                                     perm,
                                     merged);
            vpn = VirtPageNum(vpn.0 + 1);
            continue;
        }
        let ppn = frame_alloc_result().map_err(|e| LoadElfError::Mm(MmError::from(e)))?;
        zero_phys_page(ppn);
        aspace.map_page_to_ppn(vpn, ppn, perm)
              .map_err(LoadElfError::Mm)?;
        vpn = VirtPageNum(vpn.0 + 1);
    }

    // 第二遍：逐页把文件字节或零写入已映射物理页（依赖恒等/可写物理访问）。
    let mut vpn = va_start.floor_page();
    while vpn.0 < vpn_end.0 {
        let page_va = vpn.start_addr().0;
        let pb = aspace.translate_addr(vpn.start_addr())
                       .map_err(LoadElfError::Mm)?
                       .ok_or(LoadElfError::Mm(MmError::NotMapped))?
                       .0;
        for off in 0..PAGE_SIZE {
            let cur = page_va + off;
            if cur < vbase || cur >= vbase + memsz {
                continue;
            }
            let rel = cur - vbase;
            let dst = pb + off;
            if rel < filesz {
                let fi = fo + rel;
                let b = if fi < file.len() { file[fi] } else { 0 };
                unsafe {
                    (dst as *mut u8).write_volatile(b);
                }
            } else {
                unsafe {
                    (dst as *mut u8).write_volatile(0);
                }
            }
        }
        vpn = VirtPageNum(vpn.0 + 1);
    }
    Ok(())
}

/// 为单个 `PT_LOAD` 分配/合并映射，并按页从根卷读取文件内容。
///
/// LoongArch 测试盘里的 busybox 明显大于 RISC-V 版本；整文件 `Vec` 会要求
/// 4 MiB 级连续内核堆块。本路径把 ELF 数据直接写入已映射物理页，只保留小的
/// ELF 头/程序头缓冲。
fn map_segment_from_path<A : AddressSpaceOps>(aspace : &mut A,
                                              path : &str,
                                              p_vaddr : u64,
                                              p_offset : u64,
                                              p_filesz : u64,
                                              p_memsz : u64,
                                              perm : PagePerm)
                                              -> Result<(), LoadElfError> {
    let vbase = p_vaddr as usize;
    let memsz = p_memsz as usize;
    let filesz = p_filesz as usize;
    if memsz == 0 {
        return Ok(());
    }
    if filesz > memsz {
        return Err(LoadElfError::Parse);
    }
    let fo = p_offset as usize;
    let va_start = VirtAddr(vbase);
    let va_end = VirtAddr(vbase.checked_add(memsz)
                               .ok_or(LoadElfError::Parse)?);
    let mut vpn = va_start.floor_page();
    let vpn_end = va_end.ceil_page();
    while vpn.0 < vpn_end.0 {
        if let Some(_pa) = aspace.translate_addr(vpn.start_addr())
                                 .map_err(LoadElfError::Mm)?
        {
            if vpn.start_addr().0 >= 0x9000_0000 {
                runtime::logging::trace!("[elf-load] PT_LOAD refuse overlap with kernel identity \
                                          VPN={:#x}",
                                         vpn.0);
                return Err(LoadElfError::Parse);
            }
            let old = aspace.leaf_page_perm(vpn)
                            .map_err(LoadElfError::Mm)?
                            .unwrap_or(PagePerm::empty());
            let merged = old | perm;
            aspace.protect_page(vpn, merged)
                  .map_err(LoadElfError::Mm)?;
            runtime::logging::trace!("[elf-load] PT_LOAD shared VPN={:#x} merge perm {:?}|{:?} \
                                      -> {:?}",
                                     vpn.0,
                                     old,
                                     perm,
                                     merged);
            vpn = VirtPageNum(vpn.0 + 1);
            continue;
        }
        let ppn = frame_alloc_result().map_err(|e| LoadElfError::Mm(MmError::from(e)))?;
        zero_phys_page(ppn);
        aspace.map_page_to_ppn(vpn, ppn, perm)
              .map_err(LoadElfError::Mm)?;
        vpn = VirtPageNum(vpn.0 + 1);
    }

    let mut vpn = va_start.floor_page();
    while vpn.0 < vpn_end.0 {
        let page_va = vpn.start_addr().0;
        let page_end = page_va + PAGE_SIZE;
        let seg_start = cmp::max(page_va, vbase);
        let seg_end = cmp::min(page_end, vbase + memsz);
        if seg_start >= seg_end {
            vpn = VirtPageNum(vpn.0 + 1);
            continue;
        }

        let pb = aspace.translate_addr(vpn.start_addr())
                       .map_err(LoadElfError::Mm)?
                       .ok_or(LoadElfError::Mm(MmError::NotMapped))?
                       .0;
        let file_end = vbase + filesz;
        let copy_start = seg_start;
        let copy_end = cmp::min(seg_end, file_end);
        if copy_start < copy_end {
            let dst_off = copy_start - page_va;
            let rel = copy_start - vbase;
            let len = copy_end - copy_start;
            let dst = unsafe { core::slice::from_raw_parts_mut((pb + dst_off) as *mut u8, len) };
            read_path_exact(path, (fo + rel) as u64, dst)?;
        }

        let zero_start = cmp::max(seg_start, file_end);
        if zero_start < seg_end {
            unsafe {
                core::ptr::write_bytes((pb + zero_start - page_va) as *mut u8,
                                       0,
                                       seg_end - zero_start);
            }
        }
        vpn = VirtPageNum(vpn.0 + 1);
    }
    Ok(())
}

fn map_load_segments_from_path_at<A : AddressSpaceOps>(aspace : &mut A,
                                                       path : &str,
                                                       phdrs : &[u8],
                                                       phentsize : usize,
                                                       phnum : usize,
                                                       load_bias : usize)
                                                       -> Result<(usize, usize), LoadElfError> {
    let mut min_vaddr = usize::MAX;
    let mut max_vaddr = 0usize;
    for i in 0..phnum {
        let ph = i * phentsize;
        if ph + phentsize > phdrs.len() {
            return Err(LoadElfError::Parse);
        }
        if rd_u32(phdrs, ph).ok_or(LoadElfError::Parse)? != PT_LOAD {
            continue;
        }
        let p_flags = rd_u32(phdrs, ph + 4).ok_or(LoadElfError::Parse)?;
        let p_offset = rd_u64(phdrs, ph + 8).ok_or(LoadElfError::Parse)?;
        let p_vaddr = rd_u64(phdrs, ph + 16).ok_or(LoadElfError::Parse)? as usize;
        let p_filesz = rd_u64(phdrs, ph + 32).ok_or(LoadElfError::Parse)?;
        let p_memsz = rd_u64(phdrs, ph + 40).ok_or(LoadElfError::Parse)?;
        let biased_vaddr = load_bias.checked_add(p_vaddr)
                                    .ok_or(LoadElfError::Parse)?;
        map_segment_from_path(aspace,
                              path,
                              biased_vaddr as u64,
                              p_offset,
                              p_filesz,
                              p_memsz,
                              perm_from_pf(p_flags))?;
        let end = biased_vaddr.checked_add(p_memsz as usize)
                              .ok_or(LoadElfError::Parse)?;
        min_vaddr = cmp::min(min_vaddr, biased_vaddr);
        max_vaddr = cmp::max(max_vaddr, end);
    }
    if min_vaddr == usize::MAX {
        Err(LoadElfError::Parse)
    } else {
        Ok((min_vaddr, max_vaddr))
    }
}

fn read_mapped_u32<A : AddressSpaceOps>(aspace : &A, va : usize) -> Result<u32, LoadElfError> {
    let pa = aspace.translate_addr(VirtAddr(va))
                   .map_err(LoadElfError::Mm)?
                   .ok_or(LoadElfError::Mm(MmError::NotMapped))?
                   .0;
    let bytes = unsafe { core::slice::from_raw_parts(pa as *const u8, 4) };
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

fn write_mapped_u32<A : AddressSpaceOps>(aspace : &A,
                                         va : usize,
                                         insn : u32)
                                         -> Result<(), LoadElfError> {
    let pa = aspace.translate_addr(VirtAddr(va))
                   .map_err(LoadElfError::Mm)?
                   .ok_or(LoadElfError::Mm(MmError::NotMapped))?
                   .0;
    for (i, byte) in insn.to_le_bytes()
                         .iter()
                         .enumerate()
    {
        unsafe {
            ((pa + i) as *mut u8).write_volatile(*byte);
        }
    }
    Ok(())
}

fn loongarch_li_w_a7_imm(imm : u32) -> u32 { 0x0380_000b | ((imm & 0xfff) << 10) }

fn patch_loongarch_musl_sched_stubs<A : AddressSpaceOps>(aspace : &A,
                                                         interp_path : &str,
                                                         interp_base : usize)
                                                         -> Result<(), LoadElfError> {
    if interp_path != MUSL_LIBC_PATH {
        return Ok(());
    }

    for patch in MUSL_SCHED_STUB_PATCHES {
        let va = interp_base.checked_add(patch.offset)
                            .ok_or(LoadElfError::Parse)?;
        let marker = read_mapped_u32(aspace, va + 4)?;
        if marker != LOONGARCH_MUSL_SCHED_STUB_MARKER {
            runtime::logging::warn!("[elf-load] skip musl sched shim {} at {:#x}: marker \
                                     {:#x} != expected {:#x}",
                                    patch.name,
                                    va,
                                    marker,
                                    LOONGARCH_MUSL_SCHED_STUB_MARKER);
            continue;
        }

        write_mapped_u32(aspace, va, loongarch_li_w_a7_imm(patch.syscall_nr))?;
        write_mapped_u32(aspace, va + 4, LOONGARCH_INSN_SYSCALL)?;
        write_mapped_u32(aspace, va + 8, LOONGARCH_INSN_SLLI_W_A0_A0_0)?;
        write_mapped_u32(aspace, va + 12, LOONGARCH_INSN_RET)?;
        runtime::logging::debug!("[elf-load] patched loongarch musl {} stub at {:#x} -> syscall \
                                  {}",
                                 patch.name,
                                 va,
                                 patch.syscall_nr);
    }
    Ok(())
}

/// 为用户栈区间 `[stack_top - stack_size, stack_top)` 分配匿名帧并映射为
/// `R|W|U`。
fn map_user_stack<A : AddressSpaceOps>(aspace : &mut A,
                                       stack_top : usize,
                                       stack_size : usize)
                                       -> Result<(), LoadElfError> {
    let bottom = stack_top - stack_size;
    let premap_bytes = cmp::min(stack_size,
                                USER_STACK_PREMAP_PAGES.saturating_mul(PAGE_SIZE));
    let premap_bottom = stack_top.saturating_sub(premap_bytes)
                                 .max(bottom);
    let mut vpn = VirtAddr(premap_bottom).floor_page();
    let vpn_end = VirtAddr(stack_top).ceil_page();
    while vpn.0 < vpn_end.0 {
        let ppn = frame_alloc_result().map_err(|e| LoadElfError::Mm(MmError::from(e)))?;
        zero_phys_page(ppn);
        aspace.map_page_to_ppn(vpn,
                               ppn,
                               PagePerm::R | PagePerm::W | PagePerm::U)
              .map_err(LoadElfError::Mm)?;
        vpn = VirtPageNum(vpn.0 + 1);
    }
    // 栈顶再映射一页，避免测程将 SP 顶到 `stack_top` 时立即缺页。
    let ppn = frame_alloc_result().map_err(|e| LoadElfError::Mm(MmError::from(e)))?;
    zero_phys_page(ppn);
    aspace.map_page_to_ppn(vpn_end,
                           ppn,
                           PagePerm::R | PagePerm::W | PagePerm::U)
          .map_err(LoadElfError::Mm)?;
    map_signal_trampoline(aspace, VirtPageNum(vpn_end.0 + 1))?;
    Ok(())
}

fn map_signal_trampoline<A : AddressSpaceOps>(aspace : &mut A,
                                              vpn : VirtPageNum)
                                              -> Result<(), LoadElfError> {
    const CODE : [u8; 8] = [
        0x0b, 0x2c, 0xc2, 0x02, // addi.d $a7, $zero, 139
        0x00, 0x00, 0x2b, 0x00, // syscall 0
    ];
    let ppn = frame_alloc_result().map_err(|e| LoadElfError::Mm(MmError::from(e)))?;
    zero_phys_page(ppn);
    let dst = ppn.start_addr().0 as *mut u8;
    unsafe {
        core::ptr::copy_nonoverlapping(CODE.as_ptr(), dst, CODE.len());
    }
    aspace.map_page_to_ppn(vpn,
                           ppn,
                           PagePerm::R | PagePerm::X | PagePerm::U)
          .map_err(LoadElfError::Mm)
}

/// 映射完成后核对 entry 处指令与缓冲区一致，捕获「头合法、体损坏」的读损。
fn verify_mapped_entry(aspace : &LoongArch64AddressSpace,
                       entry_pc : usize,
                       data : &[u8])
                       -> Result<(), LoadElfError> {
    let fo = entry_file_offset(data, entry_pc).ok_or(LoadElfError::Parse)?;
    if fo + 4 > data.len() {
        return Err(LoadElfError::Parse);
    }
    let expected = &data[fo..fo + 4];
    let pa = aspace.translate_addr(VirtAddr(entry_pc))
                   .map_err(LoadElfError::Mm)?
                   .ok_or(LoadElfError::Parse)?
                   .0;
    let mapped = unsafe { core::slice::from_raw_parts(pa as *const u8, 4) };
    if mapped != expected {
        runtime::logging::warn!("[elf-load] abort: entry {:#x} mapped insn {:02x?} != file \
                                 {:02x?}",
                                entry_pc,
                                mapped,
                                expected);
        return Err(LoadElfError::Parse);
    }
    Ok(())
}

fn entry_file_offset_from_phdrs(phdrs : &[u8],
                                e_phentsize : usize,
                                e_phnum : usize,
                                entry_pc : usize,
                                load_bias : usize)
                                -> Option<usize> {
    for i in 0..e_phnum {
        let ph = i * e_phentsize;
        if ph + e_phentsize > phdrs.len() {
            return None;
        }
        if rd_u32(phdrs, ph)? != PT_LOAD {
            continue;
        }
        let p_offset = rd_u64(phdrs, ph + 8)? as usize;
        let p_vaddr = load_bias.checked_add(rd_u64(phdrs, ph + 16)? as usize)?;
        let p_memsz = rd_u64(phdrs, ph + 40)? as usize;
        let p_end = p_vaddr.checked_add(p_memsz)?;
        if entry_pc >= p_vaddr && entry_pc < p_end {
            return p_offset.checked_add(entry_pc - p_vaddr);
        }
    }
    None
}

fn verify_mapped_entry_from_path(aspace : &LoongArch64AddressSpace,
                                 path : &str,
                                 entry_pc : usize,
                                 phdrs : &[u8],
                                 e_phentsize : usize,
                                 e_phnum : usize)
                                 -> Result<(), LoadElfError> {
    verify_mapped_entry_from_path_at(aspace,
                                     path,
                                     entry_pc,
                                     phdrs,
                                     e_phentsize,
                                     e_phnum,
                                     0)
}

fn verify_mapped_entry_from_path_at(aspace : &LoongArch64AddressSpace,
                                    path : &str,
                                    entry_pc : usize,
                                    phdrs : &[u8],
                                    e_phentsize : usize,
                                    e_phnum : usize,
                                    load_bias : usize)
                                    -> Result<(), LoadElfError> {
    let fo = entry_file_offset_from_phdrs(phdrs,
                                          e_phentsize,
                                          e_phnum,
                                          entry_pc,
                                          load_bias).ok_or(LoadElfError::Parse)?;
    let mut expected = [0u8; 4];
    read_path_exact(path, fo as u64, &mut expected)?;
    let pa = aspace.translate_addr(VirtAddr(entry_pc))
                   .map_err(LoadElfError::Mm)?
                   .ok_or(LoadElfError::Parse)?
                   .0;
    let mapped = unsafe { core::slice::from_raw_parts(pa as *const u8, 4) };
    if mapped != expected {
        runtime::logging::warn!("[elf-load] abort: entry {:#x} mapped insn {:02x?} != file \
                                 {:02x?}",
                                entry_pc,
                                mapped,
                                expected);
        return Err(LoadElfError::Parse);
    }
    Ok(())
}

fn read_interp_path(path : &str,
                    phdrs : &[u8],
                    phentsize : usize,
                    phnum : usize)
                    -> Result<Option<String>, LoadElfError> {
    for i in 0..phnum {
        let ph = i * phentsize;
        if ph + phentsize > phdrs.len() {
            return Err(LoadElfError::Parse);
        }
        if rd_u32(phdrs, ph).ok_or(LoadElfError::Parse)? != PT_INTERP {
            continue;
        }
        let offset = rd_u64(phdrs, ph + 8).ok_or(LoadElfError::Parse)?;
        let filesz = rd_u64(phdrs, ph + 32).ok_or(LoadElfError::Parse)? as usize;
        if filesz == 0 || filesz > 256 {
            return Err(LoadElfError::Parse);
        }
        let mut buf = Vec::new();
        buf.resize(filesz, 0);
        read_path_exact(path, offset, &mut buf)?;
        let nul = buf.iter()
                     .position(|b| *b == 0)
                     .unwrap_or(buf.len());
        let interp = core::str::from_utf8(&buf[..nul]).map_err(|_| LoadElfError::Parse)?;
        return Ok(Some(remap_interp_path(path, interp)));
    }
    Ok(None)
}

fn remap_interp_path(program_path : &str, interp : &str) -> String {
    let library = interp.strip_prefix("/lib64/")
                        .or_else(|| interp.strip_prefix("/lib/"));
    if let Some(name) = library {
        if program_path.starts_with("/glibc/") {
            return format!("/glibc/lib/{name}");
        }
        if program_path.starts_with("/musl/") {
            // musl 的 libc.so 同时也是动态链接器，ld-musl-* 与 libc.so 是同一文件。
            // 无论 interp 名为 ld-linux-* 还是 ld-musl-*，都统一映射到 libc.so。
            return String::from("/musl/lib/libc.so");
        }
    }
    String::from(interp)
}

/// 从已挂载根文件系统按区间读取 `path` 指向的 ELF，避免大文件整读撑爆内核堆。
pub fn from_elf_path(path : &str) -> Result<LoadedElf, LoadElfError> {
    runtime::logging::trace!("[elf-load] from_elf_path begin path={}",
                             path);
    let mut ehdr = [0u8; 64];
    read_path_exact(path, 0, &mut ehdr)?;
    if &ehdr[0..4] != b"\x7FELF" {
        runtime::logging::trace!("[elf-load] abort: BadMagic head={:02x?}",
                                 &ehdr[..4]);
        return Err(LoadElfError::BadMagic);
    }
    if ehdr.get(4) != Some(&2) {
        runtime::logging::warn!("[elf-load] BadClass ei_class={:?} path={}",
                                ehdr.get(4),
                                path);
        return Err(LoadElfError::BadClass);
    }
    if ehdr.get(5) != Some(&1) {
        runtime::logging::trace!("[elf-load] abort: BadEndian ei_data={:?}",
                                 ehdr.get(5));
        return Err(LoadElfError::BadEndian);
    }
    let e_machine = rd_u16(&ehdr, 18).ok_or(LoadElfError::TooSmall)?;
    if e_machine != EM_LOONGARCH {
        runtime::logging::trace!("[elf-load] abort: BadMachine e_machine={} (expect \
                                  EM_LOONGARCH={})",
                                 e_machine,
                                 EM_LOONGARCH);
        return Err(LoadElfError::BadMachine);
    }
    let e_entry = rd_u64(&ehdr, 0x18).ok_or(LoadElfError::TooSmall)? as usize;
    let e_phoff = rd_u64(&ehdr, 0x20).ok_or(LoadElfError::TooSmall)? as usize;
    let e_phentsize = rd_u16(&ehdr, 0x36).ok_or(LoadElfError::TooSmall)? as usize;
    let e_phnum = rd_u16(&ehdr, 0x38).ok_or(LoadElfError::TooSmall)? as usize;
    if e_phentsize < 56 || e_phnum == 0 {
        runtime::logging::trace!("[elf-load] abort: Parse bad ph e_phentsize={} e_phnum={}",
                                 e_phentsize,
                                 e_phnum);
        return Err(LoadElfError::Parse);
    }

    let phdr_len = e_phentsize.checked_mul(e_phnum)
                              .ok_or(LoadElfError::Parse)?;
    let mut phdrs = Vec::new();
    phdrs.resize(phdr_len, 0);
    read_path_exact(path, e_phoff as u64, &mut phdrs)?;

    runtime::logging::trace!("[elf-load] ehdr e_entry={:#x} e_phoff={:#x} phentsize={} phnum={}",
                             e_entry,
                             e_phoff,
                             e_phentsize,
                             e_phnum);

    let mut aspace = LoongArch64AddressSpace::new().map_err(LoadElfError::Mm)?;
    runtime::logging::trace!("[elf-load] new user aspace pgdl will be assigned after map");
    // NOTE: 内核恒等映射在 kernel_global 中有独立页表，不重复映射到用户地址空间。
    // 若映射在此处，destroy_table 无法释放这些页表帧（subtree 无 U 位），
    // 导致每次 exec 泄漏 ~2MB 页表帧 → OOM。

    let mut min_vaddr = usize::MAX;
    let mut max_vaddr = 0usize;
    for i in 0..e_phnum {
        let ph = i * e_phentsize;
        if ph + e_phentsize > phdrs.len() {
            return Err(LoadElfError::Parse);
        }
        let p_type = rd_u32(&phdrs, ph).ok_or(LoadElfError::Parse)?;
        if p_type != PT_LOAD {
            runtime::logging::trace!("[elf-load] phdr i={} p_type={} (skip non-LOAD)",
                                     i,
                                     p_type);
            continue;
        }
        let p_flags = rd_u32(&phdrs, ph + 4).ok_or(LoadElfError::Parse)?;
        let p_offset = rd_u64(&phdrs, ph + 8).ok_or(LoadElfError::Parse)?;
        let p_vaddr = rd_u64(&phdrs, ph + 16).ok_or(LoadElfError::Parse)?;
        let p_filesz = rd_u64(&phdrs, ph + 32).ok_or(LoadElfError::Parse)?;
        let p_memsz = rd_u64(&phdrs, ph + 40).ok_or(LoadElfError::Parse)?;
        let perm = perm_from_pf(p_flags);
        runtime::logging::trace!("[elf-load] PT_LOAD i={} vaddr={:#x} memsz={:#x} filesz={:#x} \
                                  off={:#x} perm={:?}",
                                 i,
                                 p_vaddr,
                                 p_memsz,
                                 p_filesz,
                                 p_offset,
                                 perm);
        map_segment_from_path(&mut aspace,
                              path,
                              p_vaddr,
                              p_offset,
                              p_filesz,
                              p_memsz,
                              perm)?;
        let base = p_vaddr as usize;
        let end = base.checked_add(p_memsz as usize)
                      .ok_or(LoadElfError::Parse)?;
        min_vaddr = cmp::min(min_vaddr, base);
        max_vaddr = cmp::max(max_vaddr, end);
    }
    if min_vaddr == usize::MAX {
        runtime::logging::trace!("[elf-load] abort: Parse no PT_LOAD segments");
        return Err(LoadElfError::Parse);
    }
    if e_entry == 0 || e_entry < min_vaddr || e_entry >= max_vaddr {
        runtime::logging::warn!("[elf-load] abort: Parse bad e_entry={:#x} image=[{:#x},{:#x})",
                                e_entry,
                                min_vaddr,
                                max_vaddr);
        return Err(LoadElfError::Parse);
    }

    const ELF_STACK_TOP : usize = LOONGARCH64_USER_STACK_TOP;
    const ELF_STACK_SIZE : usize = USER_STACK_SIZE;
    runtime::logging::trace!("[elf-load] image range [{:#x},{:#x}) mapping done; map user stack \
                              top={:#x} size={}",
                             min_vaddr,
                             max_vaddr,
                             ELF_STACK_TOP,
                             ELF_STACK_SIZE);
    map_user_stack(&mut aspace,
                   ELF_STACK_TOP,
                   ELF_STACK_SIZE)?;
    runtime::logging::trace!("[elf-load] user stack pages mapped");

    let stack_bottom = ELF_STACK_TOP - ELF_STACK_SIZE;
    let heap_start = VirtAddr(max_vaddr).ceil_page()
                                        .start_addr();
    let gap = 256usize * PAGE_SIZE;
    let brk_max = VirtAddr(stack_bottom.saturating_sub(gap));
    if brk_max.0 <= heap_start.0 {
        runtime::logging::trace!("[elf-load] abort: image/stack gap too small for brk arena");
        return Err(LoadElfError::Parse);
    }
    let mmap_base = initial_mmap_base(heap_start);
    aspace.init_user_layout(heap_start,
                            heap_start,
                            brk_max,
                            mmap_base,
                            VirtAddr(stack_bottom),
                            VirtAddr(ELF_STACK_TOP + PAGE_SIZE));

    verify_mapped_entry_from_path(&aspace,
                                  path,
                                  e_entry,
                                  &phdrs,
                                  e_phentsize,
                                  e_phnum)?;

    let mut entry_pc = e_entry;
    let program_entry = e_entry;
    let mut interp_base = 0usize;
    if let Some(interp_path) = read_interp_path(path, &phdrs, e_phentsize, e_phnum)? {
        let interp = read_elf_header_info(interp_path.as_str())?;
        map_load_segments_from_path_at(&mut aspace,
                                       interp_path.as_str(),
                                       &interp.phdrs,
                                       interp.phentsize,
                                       interp.phnum,
                                       LOONGARCH64_INTERP_BASE)?;
        interp_base = LOONGARCH64_INTERP_BASE;
        entry_pc = interp_base.checked_add(interp.entry)
                              .ok_or(LoadElfError::Parse)?;
        verify_mapped_entry_from_path_at(&aspace,
                                         interp_path.as_str(),
                                         entry_pc,
                                         &interp.phdrs,
                                         interp.phentsize,
                                         interp.phnum,
                                         interp_base)?;
        patch_loongarch_musl_sched_stubs(&aspace, interp_path.as_str(), interp_base)?;
        runtime::logging::trace!("[elf-load] interpreter path={} base={:#x} entry={:#x}",
                                 interp_path,
                                 interp_base,
                                 entry_pc);
    }

    let phdr_va = min_vaddr.saturating_add(e_phoff);
    let leaked = Box::leak(Box::new(aspace));
    let pgdl = leaked.satp_value();
    let user_aspace_ptr = leaked as *mut crate::pagetable::LoongArch64AddressSpace as usize;
    runtime::logging::trace!("[elf-load] loaded ELF entry={:#x} pgdl={:#x} image=[{:#x},{:#x}) \
                              stack=[{:#x},{:#x}) brk=[{:#x},{:#x}) mmap_arena_base={:#x} \
                              aspace_ptr={:#x}",
                             entry_pc,
                             pgdl,
                             min_vaddr,
                             max_vaddr,
                             stack_bottom,
                             ELF_STACK_TOP,
                             heap_start.0,
                             brk_max.0,
                             mmap_base.0,
                             user_aspace_ptr);
    Ok(LoadedElf { entry_pc,
                   program_entry,
                   interp_base,
                   satp : pgdl,
                   stack_bottom,
                   stack_top : ELF_STACK_TOP,
                   image_base : min_vaddr,
                   image_size : max_vaddr.saturating_sub(min_vaddr),
                   user_aspace_ptr,
                   brk_start : heap_start.0,
                   brk_current : heap_start.0,
                   brk_max : brk_max.0,
                   mmap_arena_base : mmap_base.0,
                   phdr_va,
                   phnum : e_phnum,
                   phentsize : e_phentsize })
}

/// 从已读取的字节数组和路径加载 ELF（用于 shebang 解析等场景）。
pub fn from_elf_bytes_at_path(_data : &[u8], path : &str) -> Result<LoadedElf, LoadElfError> {
    from_elf_path(path)
}

/// 解析内存中的 ELF64 小端 LoongArch 可执行文件，建立独立三级页表地址空间、映射
/// `PT_LOAD` 与用户栈，并泄漏页表对象返回 PGDL 值。
///
/// 失败时返回具体解析或 MM 错误；成功路径下根地址空间由 `Box::leak`
/// 持有直至复位。
pub fn from_elf_bytes(data : &[u8]) -> Result<LoadedElf, LoadElfError> {
    runtime::logging::trace!("[elf-load] from_elf_bytes len={}",
                             data.len());
    if data.len() < 64 {
        runtime::logging::trace!("[elf-load] abort: TooSmall len={}",
                                 data.len());
        return Err(LoadElfError::TooSmall);
    }
    if &data[0..4] != b"\x7FELF" {
        runtime::logging::trace!("[elf-load] abort: BadMagic head={:02x?}",
                                 &data[..4]);
        return Err(LoadElfError::BadMagic);
    }
    if data.get(4) != Some(&2) {
        let n = data.len().min(16);
        runtime::logging::warn!("[elf-load] BadClass ei_class={:?} len={} first{}={:02x?}",
                                data.get(4),
                                data.len(),
                                n,
                                &data[..n]);
        return Err(LoadElfError::BadClass);
    }
    if data.get(5) != Some(&1) {
        runtime::logging::trace!("[elf-load] abort: BadEndian ei_data={:?}",
                                 data.get(5));
        return Err(LoadElfError::BadEndian);
    }
    let e_machine = rd_u16(data, 18).ok_or(LoadElfError::TooSmall)?;
    if e_machine != EM_LOONGARCH {
        runtime::logging::trace!("[elf-load] abort: BadMachine e_machine={} (expect \
                                  EM_LOONGARCH={})",
                                 e_machine,
                                 EM_LOONGARCH);
        return Err(LoadElfError::BadMachine);
    }
    let e_entry = rd_u64(data, 0x18).ok_or(LoadElfError::TooSmall)? as usize;
    let e_phoff = rd_u64(data, 0x20).ok_or(LoadElfError::TooSmall)? as usize;
    let e_phentsize = rd_u16(data, 0x36).ok_or(LoadElfError::TooSmall)? as usize;
    let e_phnum = rd_u16(data, 0x38).ok_or(LoadElfError::TooSmall)? as usize;
    if e_phentsize < 56 || e_phnum == 0 {
        runtime::logging::trace!("[elf-load] abort: Parse bad ph e_phentsize={} e_phnum={}",
                                 e_phentsize,
                                 e_phnum);
        return Err(LoadElfError::Parse);
    }

    runtime::logging::trace!("[elf-load] ehdr e_entry={:#x} e_phoff={:#x} phentsize={} phnum={}",
                             e_entry,
                             e_phoff,
                             e_phentsize,
                             e_phnum);

    let mut aspace = LoongArch64AddressSpace::new().map_err(LoadElfError::Mm)?;
    runtime::logging::trace!("[elf-load] new user aspace pgdl will be assigned after map");
    // NOTE: 内核恒等映射在 kernel_global 中有独立页表，不重复映射到用户地址空间。
    // 若映射在此处，destroy_table 无法释放这些页表帧（subtree 无 U 位），
    // 导致每次 exec 泄漏 ~2MB 页表帧 → OOM。

    let mut min_vaddr = usize::MAX;
    let mut max_vaddr = 0usize;
    for i in 0..e_phnum {
        let ph = e_phoff + i * e_phentsize;
        if ph + e_phentsize > data.len() {
            return Err(LoadElfError::Parse);
        }
        let p_type = rd_u32(data, ph).ok_or(LoadElfError::Parse)?;
        if p_type != PT_LOAD {
            runtime::logging::trace!("[elf-load] phdr i={} p_type={} (skip non-LOAD)",
                                     i,
                                     p_type);
            continue;
        }
        let p_flags = rd_u32(data, ph + 4).ok_or(LoadElfError::Parse)?;
        let p_offset = rd_u64(data, ph + 8).ok_or(LoadElfError::Parse)?;
        let p_vaddr = rd_u64(data, ph + 16).ok_or(LoadElfError::Parse)?;
        let p_filesz = rd_u64(data, ph + 32).ok_or(LoadElfError::Parse)?;
        let p_memsz = rd_u64(data, ph + 40).ok_or(LoadElfError::Parse)?;
        let perm = perm_from_pf(p_flags);
        runtime::logging::trace!("[elf-load] PT_LOAD i={} vaddr={:#x} memsz={:#x} filesz={:#x} \
                                  off={:#x} perm={:?}",
                                 i,
                                 p_vaddr,
                                 p_memsz,
                                 p_filesz,
                                 p_offset,
                                 perm);
        map_segment(&mut aspace,
                    data,
                    p_vaddr,
                    p_offset,
                    p_filesz,
                    p_memsz,
                    perm)?;
        let base = p_vaddr as usize;
        let end = base + (p_memsz as usize);
        min_vaddr = cmp::min(min_vaddr, base);
        max_vaddr = cmp::max(max_vaddr, end);
    }
    if min_vaddr == usize::MAX {
        runtime::logging::trace!("[elf-load] abort: Parse no PT_LOAD segments");
        return Err(LoadElfError::Parse);
    }
    if e_entry == 0 || e_entry < min_vaddr || e_entry >= max_vaddr {
        runtime::logging::warn!("[elf-load] abort: Parse bad e_entry={:#x} image=[{:#x},{:#x})",
                                e_entry,
                                min_vaddr,
                                max_vaddr);
        return Err(LoadElfError::Parse);
    }

    // 用户栈：固定顶与 256KiB 大小（均为 4K 页的整数倍）。
    const ELF_STACK_TOP : usize = LOONGARCH64_USER_STACK_TOP;
    const ELF_STACK_SIZE : usize = USER_STACK_SIZE;
    runtime::logging::trace!("[elf-load] image range [{:#x},{:#x}) mapping done; map user stack \
                              top={:#x} size={}",
                             min_vaddr,
                             max_vaddr,
                             ELF_STACK_TOP,
                             ELF_STACK_SIZE);
    map_user_stack(&mut aspace,
                   ELF_STACK_TOP,
                   ELF_STACK_SIZE)?;
    runtime::logging::trace!("[elf-load] user stack pages mapped");

    let stack_bottom = ELF_STACK_TOP - ELF_STACK_SIZE;
    let heap_start = VirtAddr(max_vaddr).ceil_page()
                                        .start_addr();
    let gap = 256usize * PAGE_SIZE;
    let brk_max = VirtAddr(stack_bottom.saturating_sub(gap));
    if brk_max.0 <= heap_start.0 {
        runtime::logging::trace!("[elf-load] abort: image/stack gap too small for brk arena");
        return Err(LoadElfError::Parse);
    }
    let mmap_base = initial_mmap_base(heap_start);
    aspace.init_user_layout(heap_start,
                            heap_start,
                            brk_max,
                            mmap_base,
                            VirtAddr(stack_bottom),
                            VirtAddr(ELF_STACK_TOP + PAGE_SIZE));

    verify_mapped_entry(&aspace, e_entry, data)?;

    let phdr_va = min_vaddr.saturating_add(e_phoff);
    let leaked = Box::leak(Box::new(aspace));
    let pgdl = leaked.satp_value();
    let user_aspace_ptr = leaked as *mut crate::pagetable::LoongArch64AddressSpace as usize;
    runtime::logging::trace!("[elf-load] loaded ELF entry={:#x} pgdl={:#x} image=[{:#x},{:#x}) \
                              stack=[{:#x},{:#x}) brk=[{:#x},{:#x}) mmap_arena_base={:#x} \
                              aspace_ptr={:#x}",
                             e_entry,
                             pgdl,
                             min_vaddr,
                             max_vaddr,
                             stack_bottom,
                             ELF_STACK_TOP,
                             heap_start.0,
                             brk_max.0,
                             mmap_base.0,
                             user_aspace_ptr);
    Ok(LoadedElf { entry_pc : e_entry,
                   program_entry : e_entry,
                   interp_base : 0,
                   satp : pgdl, // `satp` 字段名保留，实际内容为 LoongArch PGDL 值
                   stack_bottom,
                   stack_top : ELF_STACK_TOP,
                   image_base : min_vaddr,
                   image_size : max_vaddr.saturating_sub(min_vaddr),
                   user_aspace_ptr,
                   brk_start : heap_start.0,
                   brk_current : heap_start.0,
                   brk_max : brk_max.0,
                   mmap_arena_base : mmap_base.0,
                   phdr_va,
                   phnum : e_phnum,
                   phentsize : e_phentsize })
}

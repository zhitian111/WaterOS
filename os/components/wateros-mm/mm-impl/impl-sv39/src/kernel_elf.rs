//! 从根文件系统装载 RISC-V ELF64（小端），建立独立用户地址空间并映射 `PT_LOAD`
//! 与用户栈（分页格式由 mm-impl 完成，当前为 Sv39）。
//!
//! 用户地址空间只保留 trap 入口切回内核页表所需的最小 trampoline 映射；完整内核
//! RAM 恒等映射仅存在于内核页表中，避免污染合法用户 VA 空间。

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
#[cfg(feature = "vfs-root-read")]
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::cmp;

use api_v0::addr::{VirtAddr, VirtPageNum, PAGE_SIZE};
use api_v0::address_space::AddressSpaceOps;
use api_v0::error::{MmError, MmResult};
use api_v0::kernel_bringup::{LoadElfError, LoadedElf, RootVolumeReadError};
use api_v0::mmap::DemandPageLoader;
#[cfg(feature = "elf-lazy-map")]
use api_v0::mmap::PageFaultAccess;
use api_v0::perm::PagePerm;
use frame_alloctor::frame_alloc_result;
#[cfg(not(feature = "vfs-root-read"))]
use fs::api::{FsError, SharedFs};
use impl_common::{
    entry_file_offset, finalize_elf_read, rd_u16, rd_u32, rd_u64, ElfSegmentLoadParams, VmaBacking,
    PT_LOAD,
};

use crate::pagetable::Sv39AddressSpace;

#[cfg(feature = "vfs-root-read")]
use spin::Mutex;
#[cfg(feature = "vfs-root-read")]
use vfs::api::{SingleRootReadView, VfsError, VfsIoHandle, VfsOpenFlags, VfsOpenOps};

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
        FsError::NotEmpty => RootVolumeReadError::Unsupported,
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
        VfsError::NotEmpty => RootVolumeReadError::Unsupported,
        VfsError::NotUtf8 => RootVolumeReadError::NotUtf8,
        VfsError::NotDirectory | VfsError::TooManySymlinks | VfsError::Unsupported => {
            RootVolumeReadError::Unsupported
        }
        VfsError::Driver => RootVolumeReadError::Driver,
        VfsError::Corrupt => RootVolumeReadError::Corrupt,
        VfsError::Io => RootVolumeReadError::Io,
        VfsError::BadFd |
        VfsError::Busy |
        VfsError::WouldBlock |
        VfsError::Interrupted |
        VfsError::BrokenPipe |
        VfsError::NoDevice |
        VfsError::NoTask |
        VfsError::TooManyOpenFiles |
        VfsError::NoSpace |
        VfsError::NoMemory |
        VfsError::ReadOnlyFs |
        VfsError::AccessDenied |
        VfsError::OperationNotPermitted => RootVolumeReadError::Unsupported,
    }
}

const EM_RISCV : u16 = 243;
const ET_EXEC : u16 = 2;
const ET_DYN : u16 = 3;
const PT_INTERP : u32 = 3;
/// PIE 主程序不能装载到零地址：除空指针保护外，一些 libc/procps 也会拒绝
/// 位于低地址的全局对象。动态链接器使用另一固定区域，不与这里重叠。
const USER_PIE_BASE : usize = 0x0040_0000;
const RISCV64_INTERP_BASE : usize = 0x0000_0000_7000_0000;
/// 用户栈固定顶与大小（2 MiB；libc-bench regex 回溯需更大栈）。
pub(crate) const ELF_STACK_TOP : usize = 0x0000_0000_7FFF_A000;
pub(crate) const ELF_STACK_SIZE : usize = 2 * 1024 * 1024;
const USER_STACK_PREMAP_PAGES : usize = 16;
const PREFERRED_MMAP_BASE : usize = 0x1000_0000;
const USER_HEAP_MMAP_GAP : usize = 64 * 1024 * 1024;

fn executable_load_bias(e_type : u16) -> Result<usize, LoadElfError> {
    match e_type {
        ET_EXEC => Ok(0),
        ET_DYN => Ok(USER_PIE_BASE),
        _ => Err(LoadElfError::Parse),
    }
}

unsafe extern "C" {
    static __alltraps: u8;
    static __wateros_riscv_restore_user_from_frame: u8;
    static __wateros_riscv_kernel_satp: usize;
    static __wateros_riscv_return_frame: u8;
}

struct ElfHeaderInfo {
    entry : usize,
    phoff : usize,
    phentsize : usize,
    phnum : usize,
    phdrs : Vec<u8>,
}

fn parse_elf_header(data : &[u8]) -> Result<ElfHeaderInfo, LoadElfError> {
    if data.len() < 64 {
        return Err(LoadElfError::TooSmall);
    }
    if &data[0..4] != b"\x7FELF" {
        return Err(LoadElfError::BadMagic);
    }
    if data.get(4) != Some(&2) {
        return Err(LoadElfError::BadClass);
    }
    if data.get(5) != Some(&1) {
        return Err(LoadElfError::BadEndian);
    }
    if rd_u16(data, 18).ok_or(LoadElfError::TooSmall)? != EM_RISCV {
        return Err(LoadElfError::BadMachine);
    }
    let entry = rd_u64(data, 0x18).ok_or(LoadElfError::TooSmall)? as usize;
    let phoff = rd_u64(data, 0x20).ok_or(LoadElfError::TooSmall)? as usize;
    let phentsize = rd_u16(data, 0x36).ok_or(LoadElfError::TooSmall)? as usize;
    let phnum = rd_u16(data, 0x38).ok_or(LoadElfError::TooSmall)? as usize;
    if phentsize < 56 || phnum == 0 {
        return Err(LoadElfError::Parse);
    }
    let phdr_len = phentsize.checked_mul(phnum)
                            .ok_or(LoadElfError::Parse)?;
    if phoff.checked_add(phdr_len)
            .ok_or(LoadElfError::Parse)? >
       data.len()
    {
        return Err(LoadElfError::Parse);
    }
    let mut phdrs = Vec::new();
    phdrs.resize(phdr_len, 0);
    phdrs.copy_from_slice(&data[phoff..phoff + phdr_len]);
    Ok(ElfHeaderInfo { entry,
                       phoff,
                       phentsize,
                       phnum,
                       phdrs })
}

fn remap_interp_path(program_path : &str, interp : &str) -> String {
    let library = interp.strip_prefix("/lib/")
                        .or_else(|| interp.strip_prefix("/lib64/"));
    if let Some(name) = library {
        if program_path.starts_with("/glibc/") {
            return format!("/glibc/lib/{name}");
        }
        if program_path.starts_with("/musl/") {
            // musl 的 libc.so 同时也是动态链接器。
            return String::from("/musl/lib/libc.so");
        }
    }
    String::from(interp)
}

fn resolve_interp_path(program_path : &str, interp : &str) -> Result<String, LoadElfError> {
    let remapped = remap_interp_path(program_path, interp);
    resolve_elf_path(remapped.as_str())
}

fn read_interp_path(data : &[u8],
                    program_path : &str,
                    header : &ElfHeaderInfo)
                    -> Result<Option<String>, LoadElfError> {
    for i in 0..header.phnum {
        let ph = header.phoff + i * header.phentsize;
        if rd_u32(data, ph).ok_or(LoadElfError::Parse)? != PT_INTERP {
            continue;
        }
        let offset = rd_u64(data, ph + 8).ok_or(LoadElfError::Parse)? as usize;
        let filesz = rd_u64(data, ph + 32).ok_or(LoadElfError::Parse)? as usize;
        if filesz == 0 || filesz > 256 {
            return Err(LoadElfError::Parse);
        }
        let end = offset.checked_add(filesz)
                        .ok_or(LoadElfError::Parse)?;
        let bytes = data.get(offset..end)
                        .ok_or(LoadElfError::Parse)?;
        let nul = bytes.iter()
                       .position(|byte| *byte == 0)
                       .unwrap_or(bytes.len());
        let interp = core::str::from_utf8(&bytes[..nul]).map_err(|_| LoadElfError::Parse)?;
        return resolve_interp_path(program_path, interp).map(Some);
    }
    Ok(None)
}

/// 从根 RO 句柄读整文件；ELF 路径双读校验（见 [`finalize_elf_read`]）。
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
    let path = resolve_elf_path(path)?;
    #[cfg(feature = "vfs-root-read")]
    {
        let view = vfs::root::read_view();
        read_whole_file_ro_retry_bad_prefix_vfs(view, path.as_str())
    }
    #[cfg(not(feature = "vfs-root-read"))]
    {
        let root = fs::rootfs::active_impl::root_fs().ok_or_else(|| {
                       runtime::logging::trace!("[elf-load] abort: no root_fs (mount/driver?)");
                       LoadElfError::NoRootFs
                   })?;
        read_whole_file_ro_retry_bad_prefix(&root, path.as_str())
    }
}

pub(crate) fn resolve_elf_path(path : &str) -> Result<String, LoadElfError> {
    vfs::resolve_symlink_absolute(path, vfs::api::FinalSymlink::Follow)
        .map_err(|error| LoadElfError::RootVolume(map_vfs_to_root_vol(error)))
}

pub(crate) fn read_path_range(path : &str,
                              offset : u64,
                              buf : &mut [u8])
                              -> Result<usize, LoadElfError> {
    #[cfg(feature = "vfs-root-read")]
    {
        let view = vfs::root::read_view();
        view.read_range(path, offset, buf)
            .map_err(|e| LoadElfError::RootVolume(map_vfs_to_root_vol(e)))
    }
    #[cfg(not(feature = "vfs-root-read"))]
    {
        let root = fs::rootfs::active_impl::root_fs().ok_or(LoadElfError::NoRootFs)?;
        let g = root.lock();
        g.read_range(path, offset, buf)
         .map_err(|e| LoadElfError::RootVolume(map_fs_to_root_vol(e)))
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

#[cfg(feature = "vfs-root-read")]
fn read_whole_file_ro_retry_bad_prefix_vfs(view : &dyn SingleRootReadView,
                                           path : &str)
                                           -> Result<Vec<u8>, LoadElfError> {
    let read_once = || {
        view.read(path)
            .map_err(|e| {
                runtime::logging::error!("[elf-load] Vfs::read err={:?} path={}",
                                         e,
                                         path);
                LoadElfError::RootVolume(map_vfs_to_root_vol(e))
            })
    };
    let first = read_once()?;
    finalize_elf_read(path, first, read_once)
}

// ELF `p_flags`：bit2=R，bit1=W，bit0=X；全无时补 `R`
// 以便映射可读（与常见加载器行为一致）。
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

fn map_kernel_trampoline_page<A : AddressSpaceOps>(aspace : &mut A,
                                                   va : usize,
                                                   perm : PagePerm)
                                                   -> Result<(), LoadElfError> {
    let vpn = VirtAddr(va).floor_page();
    let ppn = vpn.to_phys_page_identity();
    if aspace.translate_addr(vpn.start_addr())
             .map_err(LoadElfError::Mm)?
             .is_some()
    {
        let old = aspace.leaf_page_perm(vpn)
                        .map_err(LoadElfError::Mm)?
                        .ok_or(LoadElfError::Parse)?;
        if old.user() {
            return Err(LoadElfError::Parse);
        }
        aspace.protect_page(vpn, old | perm)
              .map_err(LoadElfError::Mm)?;
        return Ok(());
    }
    aspace.map_page_to_ppn(vpn, ppn, perm)
          .map_err(LoadElfError::Mm)
}

/// 用户页表中的唯一内核窗口：trap 入口代码页与内核 `satp` 槽位页。
fn map_kernel_trampoline_window<A : AddressSpaceOps>(aspace : &mut A) -> Result<(), LoadElfError> {
    let trap_entry = core::ptr::addr_of!(__alltraps) as usize;
    let trap_return = core::ptr::addr_of!(__wateros_riscv_restore_user_from_frame) as usize;
    let satp_slot = core::ptr::addr_of!(__wateros_riscv_kernel_satp) as usize;
    let return_frame = core::ptr::addr_of!(__wateros_riscv_return_frame) as usize;
    map_kernel_trampoline_page(aspace,
                               trap_entry,
                               PagePerm::R | PagePerm::X)?;
    map_kernel_trampoline_page(aspace,
                               trap_return,
                               PagePerm::R | PagePerm::X)?;
    map_kernel_trampoline_page(aspace, satp_slot, PagePerm::R)?;
    map_kernel_trampoline_page(aspace,
                               return_frame,
                               PagePerm::R | PagePerm::W)?;
    Ok(())
}

/// 为单个 `PT_LOAD`
/// 分配/合并映射并填充内容：先按页建立映射，再第二遍按字节写入文件或 BSS 零。
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
            // 与上一段 PT_LOAD 共享页（例如 text/data 尾与 .bss 头同
            // VPN）：合并权限，勿再分配帧。
            let old = aspace.leaf_page_perm(vpn)
                            .map_err(LoadElfError::Mm)?
                            .unwrap_or(PagePerm::empty());
            if !old.user() {
                runtime::logging::trace!("[elf-load] PT_LOAD refuse overlap with kernel \
                                          trampoline VPN={:#x}",
                                         vpn.0);
                return Err(LoadElfError::Parse);
            }
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
        crate::pagetable::zero_phys_page(ppn);
        aspace.map_page_to_ppn(vpn, ppn, perm)
              .map_err(LoadElfError::Mm)?;
        //#runtime::logging::info!(
        //    "[elf-load] PT_LOAD alloc-map VPN={:/x} PPN={:#x} vbase={:#x}",
        //    vpn.0,
        //    ppn.0,
        //    vbase
        //);
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

fn map_load_segments_at<A : AddressSpaceOps>(aspace : &mut A,
                                             data : &[u8],
                                             header : &ElfHeaderInfo,
                                             load_bias : usize)
                                             -> Result<(usize, usize), LoadElfError> {
    let mut min_vaddr = usize::MAX;
    let mut max_vaddr = 0usize;
    for i in 0..header.phnum {
        let ph = header.phoff + i * header.phentsize;
        if rd_u32(data, ph).ok_or(LoadElfError::Parse)? != PT_LOAD {
            continue;
        }
        let p_flags = rd_u32(data, ph + 4).ok_or(LoadElfError::Parse)?;
        let p_offset = rd_u64(data, ph + 8).ok_or(LoadElfError::Parse)?;
        let p_vaddr = rd_u64(data, ph + 16).ok_or(LoadElfError::Parse)? as usize;
        let p_filesz = rd_u64(data, ph + 32).ok_or(LoadElfError::Parse)?;
        let p_memsz = rd_u64(data, ph + 40).ok_or(LoadElfError::Parse)?;
        let biased_vaddr = load_bias.checked_add(p_vaddr)
                                    .ok_or(LoadElfError::Parse)?;
        map_segment(aspace,
                    data,
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

fn entry_file_offset_at(data : &[u8],
                        header : &ElfHeaderInfo,
                        entry_pc : usize,
                        load_bias : usize)
                        -> Option<usize> {
    for i in 0..header.phnum {
        let ph = header.phoff + i * header.phentsize;
        if rd_u32(data, ph)? != PT_LOAD {
            continue;
        }
        let p_offset = rd_u64(data, ph + 8)? as usize;
        let p_vaddr = load_bias.checked_add(rd_u64(data, ph + 16)? as usize)?;
        let p_memsz = rd_u64(data, ph + 40)? as usize;
        let p_end = p_vaddr.checked_add(p_memsz)?;
        if entry_pc >= p_vaddr && entry_pc < p_end {
            return p_offset.checked_add(entry_pc - p_vaddr);
        }
    }
    None
}

fn verify_mapped_entry_at(aspace : &Sv39AddressSpace,
                          entry_pc : usize,
                          data : &[u8],
                          header : &ElfHeaderInfo,
                          load_bias : usize)
                          -> Result<(), LoadElfError> {
    let offset =
        entry_file_offset_at(data, header, entry_pc, load_bias).ok_or(LoadElfError::Parse)?;
    let expected = data.get(offset..offset + 4)
                       .ok_or(LoadElfError::Parse)?;
    let pa = aspace.translate_addr(VirtAddr(entry_pc))
                   .map_err(LoadElfError::Mm)?
                   .ok_or(LoadElfError::Parse)?
                   .0;
    let mapped = unsafe { core::slice::from_raw_parts(pa as *const u8, 4) };
    if mapped != expected {
        return Err(LoadElfError::Parse);
    }
    Ok(())
}

/// 为用户栈保留区顶部预映射少量匿名页；其余栈页由 page fault 按需补齐。
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
        crate::pagetable::zero_phys_page(ppn);
        aspace.map_page_to_ppn(vpn,
                               ppn,
                               PagePerm::R | PagePerm::W | PagePerm::U)
              .map_err(LoadElfError::Mm)?;
        vpn = VirtPageNum(vpn.0 + 1);
    }
    // 栈顶再映射一页，避免测程将 SP 顶到 `stack_top` 时立即缺页（`0x7fffa000+`）。
    let ppn = frame_alloc_result().map_err(|e| LoadElfError::Mm(MmError::from(e)))?;
    crate::pagetable::zero_phys_page(ppn);
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
        0x93, 0x08, 0xb0, 0x08, // addi a7, zero, 139
        0x73, 0x00, 0x00, 0x00, // ecall
    ];
    let ppn = frame_alloc_result().map_err(|e| LoadElfError::Mm(MmError::from(e)))?;
    crate::pagetable::zero_phys_page(ppn);
    let dst = ppn.start_addr().0 as *mut u8;
    unsafe {
        core::ptr::copy_nonoverlapping(CODE.as_ptr(), dst, CODE.len());
    }
    aspace.map_page_to_ppn(vpn,
                           ppn,
                           PagePerm::R | PagePerm::X | PagePerm::U)
          .map_err(LoadElfError::Mm)
}

/// 映射完成后核对 entry 处指令与缓冲区一致，捕获「头合法、体损坏」的 ext4 读损。
fn verify_mapped_entry(aspace : &Sv39AddressSpace,
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

/// 从文件路径验证 entry 处指令（4 字节）。
fn verify_mapped_entry_from_path_at(aspace : &mut Sv39AddressSpace,
                                    path : &str,
                                    entry_pc : usize,
                                    phdrs : &[u8],
                                    e_phentsize : usize,
                                    e_phnum : usize,
                                    load_bias : usize)
                                    -> Result<(), LoadElfError> {
    #[cfg(feature = "elf-lazy-map")]
    prefault_elf_entry_page(aspace, entry_pc)?;
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
        return Err(LoadElfError::Parse);
    }
    Ok(())
}

fn verify_mapped_entry_from_path(aspace : &mut Sv39AddressSpace,
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

#[cfg(feature = "elf-lazy-map")]
fn prefault_elf_entry_page(aspace : &mut Sv39AddressSpace,
                           entry_pc : usize)
                           -> Result<(), LoadElfError> {
    use frame_alloctor::GlobalPhysFrameAllocator;

    let page = VirtAddr(entry_pc).floor_page()
                                 .start_addr();
    if aspace.translate_addr(page)
             .map_err(LoadElfError::Mm)?
             .is_some()
    {
        return Ok(());
    }
    let mut allocator = GlobalPhysFrameAllocator;
    if !aspace.handle_lazy_page_fault(&mut allocator,
                                      page,
                                      PageFaultAccess::Execute)
              .map_err(LoadElfError::Mm)?
    {
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

/// 从文件系统路径装载 ELF64（逐段读取，避免大文件整读撑爆内核堆）。
/// 静态链接的 busybox 可达 ~1.5MB，整读需要连续大量内核堆内存。
/// 本路径只读取 header + phdrs（仅几 KB），映射时从文件系统按页读取。
pub fn from_elf_path(path : &str) -> Result<LoadedElf, LoadElfError> {
    let resolved_path = resolve_elf_path(path)?;
    let path = resolved_path.as_str();
    runtime::logging::trace!("[elf-load] from_elf_path begin path={}",
                             path);

    // 只读 64 字节 ELF header
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
    if rd_u16(&ehdr, 18).ok_or(LoadElfError::TooSmall)? != EM_RISCV {
        return Err(LoadElfError::BadMachine);
    }
    let e_type = rd_u16(&ehdr, 16).ok_or(LoadElfError::TooSmall)?;
    let load_bias = executable_load_bias(e_type)?;
    let e_entry = rd_u64(&ehdr, 0x18).ok_or(LoadElfError::TooSmall)? as usize;
    let e_phoff = rd_u64(&ehdr, 0x20).ok_or(LoadElfError::TooSmall)? as usize;
    let e_phentsize = rd_u16(&ehdr, 0x36).ok_or(LoadElfError::TooSmall)? as usize;
    let e_phnum = rd_u16(&ehdr, 0x38).ok_or(LoadElfError::TooSmall)? as usize;
    if e_phentsize < 56 || e_phnum == 0 {
        return Err(LoadElfError::Parse);
    }

    // 只读取程序头表（不含段数据）
    let phdr_len = e_phentsize.checked_mul(e_phnum)
                              .ok_or(LoadElfError::Parse)?;
    let mut phdrs = Vec::new();
    phdrs.resize(phdr_len, 0);
    read_path_exact(path, e_phoff as u64, &mut phdrs)?;

    runtime::logging::trace!("[elf-load] e_entry={:#x} phoff={} phentsize={} phnum={}",
                             e_entry,
                             e_phoff,
                             e_phentsize,
                             e_phnum);

    let mut aspace = Sv39AddressSpace::new().map_err(LoadElfError::Mm)?;
    map_kernel_trampoline_window(&mut aspace)?;

    // 从文件系统路径映射 PT_LOAD 段，不整读文件
    let (min_vaddr, max_vaddr) = map_load_segments_from_path_at(&mut aspace,
                                                                path,
                                                                &phdrs,
                                                                e_phentsize,
                                                                e_phnum,
                                                                load_bias)?;

    let program_entry = load_bias.checked_add(e_entry)
                                 .ok_or(LoadElfError::Parse)?;

    if e_entry == 0 || program_entry < min_vaddr || program_entry >= max_vaddr {
        runtime::logging::warn!("[elf-load] bad entry={:#x} bias={:#x} image=[{:#x},{:#x})",
                                program_entry,
                                load_bias,
                                min_vaddr,
                                max_vaddr);
        return Err(LoadElfError::Parse);
    }

    map_user_stack(&mut aspace,
                   ELF_STACK_TOP,
                   ELF_STACK_SIZE)?;
    let stack_bottom = ELF_STACK_TOP - ELF_STACK_SIZE;
    let heap_start = VirtAddr(max_vaddr).ceil_page()
                                        .start_addr();
    let mmap_base = VirtAddr(cmp::max(heap_start.0
                                                .saturating_add(USER_HEAP_MMAP_GAP),
                                      PREFERRED_MMAP_BASE));
    // brk 与 mmap 使用相邻但不重叠的 arena。旧实现把 brk_max 放到栈下方，
    // 导致 mmap 先占用中间地址后 brk 才靠 VMA 冲突被动失败。
    let brk_max = mmap_base;
    if brk_max.0 <= heap_start.0 || mmap_base.0 >= stack_bottom {
        return Err(LoadElfError::Parse);
    }
    aspace.init_user_layout(heap_start,
                            heap_start,
                            brk_max,
                            mmap_base,
                            VirtAddr(stack_bottom),
                            VirtAddr(ELF_STACK_TOP + PAGE_SIZE));

    // 使用 path-based 验证
    verify_mapped_entry_from_path_at(&mut aspace,
                                     path,
                                     program_entry,
                                     &phdrs,
                                     e_phentsize,
                                     e_phnum,
                                     load_bias)?;

    // 处理动态链接器
    let mut entry_pc = program_entry;
    let mut interp_base = 0usize;
    let interp_path = read_interp_path_from_phdrs(path, &phdrs, e_phentsize, e_phnum)?;
    if let Some(interp_path) = interp_path {
        runtime::logging::trace!("[elf-load] interpreter path={}",
                                 interp_path);
        let interp_ehdr = read_elf_header_info(interp_path.as_str())?;
        map_load_segments_from_path_at(&mut aspace,
                                       interp_path.as_str(),
                                       &interp_ehdr.phdrs,
                                       interp_ehdr.phentsize,
                                       interp_ehdr.phnum,
                                       RISCV64_INTERP_BASE)?;
        interp_base = RISCV64_INTERP_BASE;
        entry_pc = RISCV64_INTERP_BASE.checked_add(interp_ehdr.entry)
                                      .ok_or(LoadElfError::Parse)?;
        verify_mapped_entry_from_path_at(&mut aspace,
                                         interp_path.as_str(),
                                         entry_pc,
                                         &interp_ehdr.phdrs,
                                         interp_ehdr.phentsize,
                                         interp_ehdr.phnum,
                                         interp_base)?;
    }

    let phdr_va = min_vaddr.saturating_add(e_phoff);
    let satp = aspace.satp_value();
    let user_aspace_ptr = crate::user_aspace::into_handle(aspace);
    runtime::logging::trace!("[elf-load] loaded ELF entry={:#x} satp={:#x} image=[{:#x},{:#x}) \
                              ...",
                             entry_pc,
                             satp,
                             min_vaddr,
                             max_vaddr);
    Ok(LoadedElf { entry_pc,
                   program_entry,
                   interp_base,
                   satp,
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

/// 解析 ELF 文件 header 信息（仅读 64 字节 + phdrs）。
fn read_elf_header_info(path : &str) -> Result<ElfHeaderInfo, LoadElfError> {
    let mut ehdr = [0u8; 64];
    read_path_exact(path, 0, &mut ehdr)?;
    if &ehdr[0..4] != b"\x7FELF" {
        return Err(LoadElfError::BadMagic);
    }
    let e_entry = rd_u64(&ehdr, 0x18).ok_or(LoadElfError::TooSmall)? as usize;
    let e_phoff = rd_u64(&ehdr, 0x20).ok_or(LoadElfError::TooSmall)? as usize;
    let e_phentsize = rd_u16(&ehdr, 0x36).ok_or(LoadElfError::TooSmall)? as usize;
    let e_phnum = rd_u16(&ehdr, 0x38).ok_or(LoadElfError::TooSmall)? as usize;
    if e_phentsize < 56 || e_phnum == 0 {
        return Err(LoadElfError::Parse);
    }
    let phdr_len = e_phentsize.checked_mul(e_phnum)
                              .ok_or(LoadElfError::Parse)?;
    let mut phdrs = Vec::new();
    phdrs.resize(phdr_len, 0);
    read_path_exact(path, e_phoff as u64, &mut phdrs)?;
    Ok(ElfHeaderInfo { entry : e_entry,
                       phoff : e_phoff,
                       phentsize : e_phentsize,
                       phnum : e_phnum,
                       phdrs })
}

/// 从程序头表中读取 PT_INTERP 路径（以文件路径中的数据段为来源）。
fn read_interp_path_from_phdrs(path : &str,
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
        return resolve_interp_path(path, interp).map(Some);
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::remap_interp_path;

    #[test]
    fn remaps_glibc_interpreter_into_bundle() {
        assert_eq!(remap_interp_path("/glibc/hackbench",
                                     "/lib/ld-linux-riscv64-lp64d.so.1"),
                   "/glibc/lib/ld-linux-riscv64-lp64d.so.1");
    }

    #[test]
    fn remaps_musl_interpreter_to_libc() {
        assert_eq!(remap_interp_path("/musl/hackbench",
                                     "/lib/ld-musl-riscv64.so.1"),
                   "/musl/lib/libc.so");
    }

    #[test]
    fn preserves_standard_root_interpreter() {
        assert_eq!(remap_interp_path("/usr/bin/sleep",
                                     "/lib/ld-linux-riscv64-lp64d.so.1"),
                   "/lib/ld-linux-riscv64-lp64d.so.1");
    }
}

struct ElfPathSegmentLoader {
    path : String,
    params : ElfSegmentLoadParams,
    shareable : bool,
    #[cfg(feature = "vfs-root-read")]
    handle : Arc<Mutex<Box<dyn VfsIoHandle>>>,
}

impl ElfPathSegmentLoader {
    fn new(path : &str,
           vbase : usize,
           p_offset : usize,
           filesz : usize,
           vma_start : usize,
           shareable : bool)
           -> Result<Self, LoadElfError> {
        let vma_file_origin = p_offset.saturating_sub(vbase.saturating_sub(vma_start));
        #[cfg(feature = "vfs-root-read")]
        let handle =
            vfs::active_impl::backend().open(path, VfsOpenFlags::read())
                                       .map_err(|error| {
                                           LoadElfError::RootVolume(map_vfs_to_root_vol(error))
                                       })?;
        Ok(Self { path : String::from(path),
                  params : ElfSegmentLoadParams { vbase,
                                                  p_offset,
                                                  filesz,
                                                  vma_start,
                                                  vma_file_origin },
                  shareable,
                  #[cfg(feature = "vfs-root-read")]
                  handle : Arc::new(Mutex::new(handle)) })
    }
}

#[cfg(feature = "vfs-root-read")]
fn read_handle_exact(handle : &Arc<Mutex<Box<dyn VfsIoHandle>>>,
                     offset : u64,
                     buf : &mut [u8])
                     -> MmResult<()> {
    let mut filled = 0usize;
    while filled < buf.len() {
        let n = handle.lock()
                      .read_at(offset + filled as u64,
                               &mut buf[filled..])
                      .map_err(|_| MmError::AccessViolation)?;
        if n == 0 {
            return Err(MmError::AccessViolation);
        }
        filled += n;
    }
    Ok(())
}

impl DemandPageLoader for ElfPathSegmentLoader {
    fn duplicate_box(&self) -> MmResult<Box<dyn DemandPageLoader>> {
        Ok(Box::new(Self { path : self.path
                                      .clone(),
                           params:
                               self.params.clone(),
                           shareable:
                               self.shareable,
                           #[cfg(feature = "vfs-root-read")]
                           handle:
                               self.handle.clone() }))
    }

    fn load_page(&mut self, file_offset : usize, dst : &mut [u8]) -> MmResult<()> {
        self.params
            .fill_page(file_offset, dst, |pos, buf| {
                #[cfg(feature = "vfs-root-read")]
                {
                    read_handle_exact(&self.handle, pos as u64, buf)
                }
                #[cfg(not(feature = "vfs-root-read"))]
                {
                    read_path_exact(&self.path, pos as u64, buf).map_err(|_| {
                                                                    MmError::AccessViolation
                                                                })
                }
            })
    }

    fn load_shared_page(&mut self,
                        file_offset : usize)
                        -> MmResult<Option<api_v0::addr::PhysPageNum>> {
        #[cfg(feature = "vfs-root-read")]
        {
            if !self.shareable {
                return Ok(None);
            }
            let Some(identity) = self.handle
                                     .lock()
                                     .file_content_identity()
            else {
                return Ok(None);
            };
            let key_params = self.params.clone();
            let load_params = key_params.clone();
            let handle = self.handle.clone();
            let ppn = impl_common::load_or_get_readonly_elf_page(
                                                                 &identity,
                                                                 &key_params,
                                                                 file_offset,
                                                                 move |dst| {
                                                                     load_params.fill_page(file_offset, dst, |pos, buf| {
                    read_handle_exact(&handle, pos as u64, buf)
                })
                                                                 },
            )?;
            Ok(Some(ppn))
        }
        #[cfg(not(feature = "vfs-root-read"))]
        {
            let _ = file_offset;
            Ok(None)
        }
    }
}

fn register_lazy_segment_run(aspace : &mut Sv39AddressSpace,
                             path : &str,
                             run_start : VirtAddr,
                             run_end : VirtAddr,
                             vbase : usize,
                             p_offset : usize,
                             filesz : usize,
                             perm : PagePerm)
                             -> Result<(), LoadElfError> {
    let vma_file_origin = p_offset.checked_sub(vbase.saturating_sub(run_start.0))
                                  .ok_or(LoadElfError::Parse)?;
    let vma_file_size = filesz.checked_add(vbase.saturating_sub(run_start.0))
                              .ok_or(LoadElfError::Parse)?;
    let loader = Box::new(ElfPathSegmentLoader::new(path,
                                                    vbase,
                                                    p_offset,
                                                    filesz,
                                                    run_start.0,
                                                    !perm.writable())?);
    aspace.register_lazy_file_vma(run_start,
                                  run_end,
                                  perm,
                                  vma_file_origin,
                                  vma_file_size,
                                  VmaBacking::File { loader })
          .map_err(LoadElfError::Mm)
}

#[cfg(feature = "elf-lazy-map")]
fn map_segment_from_path_lazy(aspace : &mut Sv39AddressSpace,
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
    let mut lazy_run_start : Option<VirtAddr> = None;

    while vpn.0 < vpn_end.0 {
        let page_va = vpn.start_addr();
        let page_end = VirtAddr(page_va.0 + PAGE_SIZE);

        if let Some(run_start) = lazy_run_start {
            if aspace.translate_addr(page_va)
                     .map_err(LoadElfError::Mm)?
                     .is_some() ||
               aspace.lazy_vma_contains(page_va)
            {
                register_lazy_segment_run(aspace, path, run_start, page_va, vbase, fo, filesz,
                                          perm)?;
                lazy_run_start = None;
            }
        }

        if let Some(_pa) = aspace.translate_addr(page_va)
                                 .map_err(LoadElfError::Mm)?
        {
            let old = aspace.leaf_page_perm(vpn)
                            .map_err(LoadElfError::Mm)?
                            .unwrap_or(PagePerm::empty());
            if !old.user() {
                return Err(LoadElfError::Parse);
            }
            let merged = old | perm;
            aspace.protect_page(vpn, merged)
                  .map_err(LoadElfError::Mm)?;
        } else if aspace.lazy_vma_contains(page_va) {
            aspace.merge_lazy_file_vma_perm(page_va, page_end, perm)
                  .map_err(LoadElfError::Mm)?;
        } else if lazy_run_start.is_none() {
            lazy_run_start = Some(page_va);
        }

        vpn = VirtPageNum(vpn.0 + 1);
    }

    if let Some(run_start) = lazy_run_start {
        register_lazy_segment_run(aspace,
                                  path,
                                  run_start,
                                  vpn_end.start_addr(),
                                  vbase,
                                  fo,
                                  filesz,
                                  perm)?;
    }
    Ok(())
}

/// 从文件系统路径映射 PT_LOAD 段（单段），避免整文件读取。
#[cfg(not(feature = "elf-lazy-map"))]
fn map_segment_from_path_eager<A : AddressSpaceOps>(aspace : &mut A,
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
            let old = aspace.leaf_page_perm(vpn)
                            .map_err(LoadElfError::Mm)?
                            .unwrap_or(PagePerm::empty());
            if !old.user() {
                return Err(LoadElfError::Parse);
            }
            let merged = old | perm;
            aspace.protect_page(vpn, merged)
                  .map_err(LoadElfError::Mm)?;
            vpn = VirtPageNum(vpn.0 + 1);
            continue;
        }
        let ppn = frame_alloc_result().map_err(|e| LoadElfError::Mm(MmError::from(e)))?;
        crate::pagetable::zero_phys_page(ppn);
        aspace.map_page_to_ppn(vpn, ppn, perm)
              .map_err(LoadElfError::Mm)?;
        vpn = VirtPageNum(vpn.0 + 1);
    }

    // 第二遍：逐页从文件系统读取。按页内连续区间一次读入，避免把大 ELF
    // 拆成数万次 64B 小 I/O。
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

fn map_segment_from_path(aspace : &mut Sv39AddressSpace,
                         path : &str,
                         p_vaddr : u64,
                         p_offset : u64,
                         p_filesz : u64,
                         p_memsz : u64,
                         perm : PagePerm)
                         -> Result<(), LoadElfError> {
    #[cfg(feature = "elf-lazy-map")]
    {
        return map_segment_from_path_lazy(aspace, path, p_vaddr, p_offset, p_filesz, p_memsz,
                                          perm);
    }
    #[cfg(not(feature = "elf-lazy-map"))]
    {
        map_segment_from_path_eager(aspace, path, p_vaddr, p_offset, p_filesz, p_memsz, perm)
    }
}

/// 从文件系统路径映射所有 PT_LOAD 段。
fn map_load_segments_from_path_at(aspace : &mut Sv39AddressSpace,
                                  path : &str,
                                  phdrs : &[u8],
                                  e_phentsize : usize,
                                  e_phnum : usize,
                                  load_bias : usize)
                                  -> Result<(usize, usize), LoadElfError> {
    let mut min_vaddr = usize::MAX;
    let mut max_vaddr = 0usize;
    for i in 0..e_phnum {
        let ph = i * e_phentsize;
        if ph + e_phentsize > phdrs.len() {
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


/// 保留整读接口供 `from_elf_bytes` 使用（boot info、静态编译 init 等）。
pub fn from_elf_bytes(data : &[u8]) -> Result<LoadedElf, LoadElfError> {
    runtime::logging::trace!("[elf-load] from_elf_bytes len={}",
                             data.len());
    if data.len() < 64 {
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
    if e_machine != EM_RISCV {
        runtime::logging::trace!("[elf-load] abort: BadMachine e_machine={} (expect EM_RISCV={})",
                                 e_machine,
                                 EM_RISCV);
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

    let mut aspace = Sv39AddressSpace::new().map_err(LoadElfError::Mm)?;
    runtime::logging::trace!("[elf-load] new user aspace satp will be assigned after map");
    map_kernel_trampoline_window(&mut aspace)?;
    runtime::logging::trace!("[elf-load] kernel trampoline window in user aspace ok");

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

    // 用户栈：固定顶与 256KiB 大小（均为 4K
    // 页的整数倍）；与具体用户镜像链接脚本无关，属 bring-up 约定。
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
    let mmap_base = VirtAddr(cmp::max(heap_start.0
                                                .saturating_add(USER_HEAP_MMAP_GAP),
                                      PREFERRED_MMAP_BASE));
    let brk_max = mmap_base;
    if brk_max.0 <= heap_start.0 || mmap_base.0 >= stack_bottom {
        runtime::logging::trace!("[elf-load] abort: image/stack gap too small for brk arena");
        return Err(LoadElfError::Parse);
    }
    aspace.init_user_layout(heap_start,
                            heap_start,
                            brk_max,
                            mmap_base,
                            VirtAddr(stack_bottom),
                            VirtAddr(ELF_STACK_TOP + PAGE_SIZE));

    verify_mapped_entry(&aspace, e_entry, data)?;

    let program_entry = e_entry;
    let entry_pc = e_entry;
    let interp_base = 0usize;
    // `from_elf_bytes` 仅用于内存中的 ELF（boot info 等），不需要动态链接器

    let phdr_va = min_vaddr.saturating_add(e_phoff);
    let satp = aspace.satp_value();
    let user_aspace_ptr = crate::user_aspace::into_handle(aspace);
    runtime::logging::trace!("[elf-load] loaded ELF entry={:#x} satp={:#x} image=[{:#x},{:#x}) \
                              stack=[{:#x},{:#x}) brk=[{:#x},{:#x}) mmap_arena_base={:#x} \
                              aspace_ptr={:#x}",
                             entry_pc,
                             satp,
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
                   satp,
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

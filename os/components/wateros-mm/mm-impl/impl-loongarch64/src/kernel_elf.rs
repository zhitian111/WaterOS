//! 从根文件系统装载 LoongArch ELF64（小端），建立独立用户地址空间并映射
//! `PT_LOAD` 与用户栈（分页格式由 mm-impl 完成，当前为 LoongArch64 三级页表）。
//!
//! 用户地址空间内 **额外** 恒等映射内核 RAM（与 [`crate::kernel_global`] 的
//! `phys_ram_end_exclusive`
//! 一致），便于同一套页表里内核辅助访问；用户段不得与用户 VPN 或 `0x9000_0000`
//! 以上恒等区非法重叠（重叠时返回 `Parse`）。

extern crate alloc;

use alloc::boxed::Box;
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

use crate::pagetable::LoongArch64AddressSpace;

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
        VfsError::BadFd
        | VfsError::WouldBlock
        | VfsError::BrokenPipe
        | VfsError::NoTask
        | VfsError::ReadOnlyFs => RootVolumeReadError::Unsupported,
    }
}

const PT_LOAD : u32 = 1;
const EM_LOONGARCH : u16 = 258;

/// 仅检查 ELF64 小端头前缀；用于在 `from_elf_path` 首读异常时决定是否重读。
#[inline]
fn elf_loongarch64_le_prefix_ok(data : &[u8]) -> bool { api_v0::executable::is_elf_prefix(data) }

/// 文本/脚本文件不需要 ELF 前缀重试（避免对 `.sh` 误报警）。
#[inline]
fn skip_elf_prefix_retry(data : &[u8]) -> bool { api_v0::executable::is_text_file(data) }

/// 从根 RO 句柄读整文件；若首读 ELF 前缀明显损坏则 **再读一次**。
#[cfg(not(feature = "vfs-root-read"))]
fn read_whole_file_ro_retry_bad_prefix(root : &SharedFs,
                                       path : &str)
                                       -> Result<Vec<u8>, LoadElfError> {
    let first = {
        let g = root.lock();
        g.read(path)
         .map_err(|e| {
             runtime::logging::trace!("[elf-load] abort: Fs::read err={:?} path={}",
                                      e,
                                      path);
             LoadElfError::RootVolume(map_fs_to_root_vol(e))
         })?
    };
    if elf_loongarch64_le_prefix_ok(&first) {
        return Ok(first);
    }
    if skip_elf_prefix_retry(&first) {
        return Ok(first);
    }
    let n = first.len().min(16);
    runtime::logging::warn!("[elf-load] first read bad ELF64-LE prefix (len={} first{}={:02x?}); \
                             retry read once path={}",
                            first.len(),
                            n,
                            &first[..n],
                            path);
    let second = {
        let g = root.lock();
        g.read(path)
         .map_err(|e| {
             runtime::logging::trace!("[elf-load] abort: Fs::read retry err={:?} path={}",
                                      e,
                                      path);
             LoadElfError::RootVolume(map_fs_to_root_vol(e))
         })?
    };
    if !elf_loongarch64_le_prefix_ok(&second) {
        let n2 = second.len().min(16);
        runtime::logging::warn!("[elf-load] retry read still bad prefix (len={} first{}={:02x?}) \
                                 path={}",
                                second.len(),
                                n2,
                                &second[..n2],
                                path);
    }
    Ok(second)
}

/// 从根卷读取 `path` 的完整字节（含 ELF 前缀损坏时的一次重试）。
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

#[cfg(feature = "vfs-root-read")]
fn read_whole_file_ro_retry_bad_prefix_vfs(view : &dyn SingleRootReadView,
                                           path : &str)
                                           -> Result<Vec<u8>, LoadElfError> {
    let first = view.read(path)
                    .map_err(|e| {
                        runtime::logging::trace!("[elf-load] abort: Vfs::read err={:?} path={}",
                                                 e,
                                                 path);
                        LoadElfError::RootVolume(map_vfs_to_root_vol(e))
                    })?;
    if elf_loongarch64_le_prefix_ok(&first) {
        return Ok(first);
    }
    if skip_elf_prefix_retry(&first) {
        return Ok(first);
    }
    let n = first.len().min(16);
    runtime::logging::warn!("[elf-load] first read bad ELF64-LE prefix (len={} first{}={:02x?}); \
                             retry read once path={}",
                            first.len(),
                            n,
                            &first[..n],
                            path);
    let second = view.read(path)
                     .map_err(|e| {
                         runtime::logging::trace!("[elf-load] abort: Vfs::read retry err={:?} \
                                                   path={}",
                                                  e,
                                                  path);
                         LoadElfError::RootVolume(map_vfs_to_root_vol(e))
                     })?;
    if !elf_loongarch64_le_prefix_ok(&second) {
        let n2 = second.len().min(16);
        runtime::logging::warn!("[elf-load] retry read still bad prefix (len={} first{}={:02x?}) \
                                 path={}",
                                second.len(),
                                n2,
                                &second[..n2],
                                path);
    }
    Ok(second)
}

/// 小端读取 `u16`；越界返回 `None`。
#[inline]
fn rd_u16(s : &[u8], o : usize) -> Option<u16> {
    s.get(o..o + 2)?
     .try_into()
     .ok()
     .map(u16::from_le_bytes)
}

/// 小端读取 `u32`。
#[inline]
fn rd_u32(s : &[u8], o : usize) -> Option<u32> {
    s.get(o..o + 4)?
     .try_into()
     .ok()
     .map(u32::from_le_bytes)
}

/// 小端读取 `u64`。
#[inline]
fn rd_u64(s : &[u8], o : usize) -> Option<u64> {
    s.get(o..o + 8)?
     .try_into()
     .ok()
     .map(u64::from_le_bytes)
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

/// 为用户栈区间 `[stack_top - stack_size, stack_top)` 分配匿名帧并映射为
/// `R|W|U`。
fn map_user_stack<A : AddressSpaceOps>(aspace : &mut A,
                                       stack_top : usize,
                                       stack_size : usize)
                                       -> Result<(), LoadElfError> {
    let bottom = stack_top - stack_size;
    let mut vpn = VirtAddr(bottom).floor_page();
    let vpn_end = VirtAddr(stack_top).ceil_page();
    while vpn.0 < vpn_end.0 {
        let ppn = frame_alloc_result().map_err(|e| LoadElfError::Mm(MmError::from(e)))?;
        aspace.map_page_to_ppn(vpn,
                               ppn,
                               PagePerm::R | PagePerm::W | PagePerm::U)
              .map_err(LoadElfError::Mm)?;
        vpn = VirtPageNum(vpn.0 + 1);
    }
    Ok(())
}

/// 从已挂载根文件系统读取 `path` 指向的 ELF，再调用 [`from_elf_bytes`]。
pub fn from_elf_path(path : &str) -> Result<LoadedElf, LoadElfError> {
    runtime::logging::trace!("[elf-load] from_elf_path begin path={}",
                             path);
    let data = read_path_bytes(path)?;
    runtime::logging::trace!("[elf-load] read ok bytes={} path={}",
                             data.len(),
                             path);
    from_elf_bytes(&data)
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
    map_kernel_ram_identity(&mut aspace)?;
    runtime::logging::trace!("[elf-load] kernel RAM identity map in user aspace ok");

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

    // 用户栈：固定顶与 16KiB 大小（均为 4K 页的整数倍）。
    const ELF_STACK_TOP : usize = 0x0000_0000_7FFF_A000;
    const ELF_STACK_SIZE : usize = 16 * 1024;
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
    const PREFERRED_MMAP_BASE : usize = 0x1000_0000;
    let mmap_base = VirtAddr(cmp::max(heap_start.0
                                                .saturating_add(PAGE_SIZE),
                                      PREFERRED_MMAP_BASE));
    aspace.init_user_layout(heap_start, heap_start, brk_max, mmap_base);

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

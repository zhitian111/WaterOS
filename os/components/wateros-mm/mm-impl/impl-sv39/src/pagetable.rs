//! Sv39 三级页表与 **4 KiB 叶子页** 实现；仅本 crate
//! 内部可见，不通过聚合层对外暴露。
//!
//! ## 物理访问假设
//!
//! [`table_mut`] 将 PPN 转为指针读写页表，要求 **PPN
//! 对应物理内存在内核视角下可直接访问** （常见 bring-up：内核恒等映射
//! RAM/MMIO）。若改为偏移映射，必须同步改写该路径。
//!
//! ## 与 trap / 访问位
//!
//! 映射时预先置 PTE **A/D** 位，避免依赖 S 态 load/store 触发页故障来置位（早期
//! bring-up 策略）。

use api_v0::addr::{PhysAddr, PhysPageNum, VirtAddr, VirtPageNum, PAGE_SIZE};
use api_v0::address_space::AddressSpaceOps;
use api_v0::error::{MmError, MmResult};
use api_v0::perm::PagePerm;

use frame_alloctor::{frame_alloc_result, frame_dealloc_result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Sv39PteFlags(u16);

impl Sv39PteFlags {
    const V : Self = Self(1 << 0);
    const R : Self = Self(1 << 1);
    const W : Self = Self(1 << 2);
    const X : Self = Self(1 << 3);
    const U : Self = Self(1 << 4);
    const A : Self = Self(1 << 6);
    const D : Self = Self(1 << 7);

    #[inline]
    const fn empty() -> Self { Self(0) }

    #[inline]
    const fn bits(self) -> u16 { self.0 }

    #[inline]
    const fn is_valid(self) -> bool { (self.0 & Self::V.0) != 0 }

    #[inline]
    const fn is_leaf(self) -> bool { (self.0 & (Self::R.0 | Self::W.0 | Self::X.0)) != 0 }

    #[inline]
    fn from_perm(perm : PagePerm) -> Self {
        let mut f = Self::empty();
        f.0 |= Self::V.0;
        if perm.readable() {
            f.0 |= Self::R.0;
        }
        if perm.writable() {
            f.0 |= Self::W.0;
        }
        if perm.executable() {
            f.0 |= Self::X.0;
        }
        if perm.user() {
            f.0 |= Self::U.0;
        }
        f.0 |= Self::A.0 | Self::D.0;
        f
    }

    #[inline]
    fn to_page_perm(self) -> PagePerm {
        let mut p = PagePerm::empty();
        if (self.0 & Self::R.0) != 0 {
            p = p | PagePerm::R;
        }
        if (self.0 & Self::W.0) != 0 {
            p = p | PagePerm::W;
        }
        if (self.0 & Self::X.0) != 0 {
            p = p | PagePerm::X;
        }
        if (self.0 & Self::U.0) != 0 {
            p = p | PagePerm::U;
        }
        p
    }
}

#[repr(transparent)]
#[derive(Clone, Copy)]
struct Sv39Pte(usize);

impl Sv39Pte {
    #[inline]
    const fn zero() -> Self { Self(0) }

    #[inline]
    fn flags(self) -> Sv39PteFlags { Sv39PteFlags((self.0 & 0x3FF) as u16) }

    #[inline]
    fn ppn(self) -> PhysPageNum { PhysPageNum((self.0 >> 10) & ((1usize << 44) - 1)) }

    #[inline]
    fn set(&mut self, ppn : PhysPageNum, flags : Sv39PteFlags) {
        self.0 = (ppn.0 << 10) | (flags.bits() as usize);
    }

    #[inline]
    fn clear(&mut self) { self.0 = 0; }
}

const SV39_LEVELS : usize = 3;
const SV39_ENTRIES : usize = 512;

#[inline]
fn vpn_indexes(vpn : VirtPageNum) -> [usize; 3] {
    let v = vpn.0;
    [(v >> 0) & 0x1FF, (v >> 9) & 0x1FF, (v >> 18) & 0x1FF]
}

/// # Safety
///
/// 调用方保证 `ppn` 指向已映射且 **4 KiB 对齐** 的页表存储。
#[inline]
unsafe fn table_mut(ppn : PhysPageNum) -> &'static mut [Sv39Pte; SV39_ENTRIES] {
    let pa = ppn.0 * PAGE_SIZE;
    unsafe { &mut *(pa as *mut [Sv39Pte; SV39_ENTRIES]) }
}

#[inline]
fn alloc_table_frame_zeroed() -> MmResult<PhysPageNum> {
    let ppn = frame_alloc_result().map_err(MmError::from)?;
    unsafe {
        let tbl = table_mut(ppn);
        for e in tbl.iter_mut() {
            *e = Sv39Pte::zero();
        }
    }
    Ok(ppn)
}

/// Sv39 根页表与 walk 状态；所有映射均为 **4 KiB 叶子**。
pub struct Sv39AddressSpace {
    root : PhysPageNum,
    /// 用户堆起点（页对齐，位于 ELF 镜像尾之后）。
    pub(crate) user_brk_start : VirtAddr,
    /// 当前 program break（堆尾上界，可非页对齐）。
    pub(crate) user_brk_current_end : VirtAddr,
    /// `brk` 允许增长到的最大虚拟地址（不含）。
    pub(crate) user_brk_max : VirtAddr,
    /// 匿名 `mmap` bump 指针（下一段匿名映射的起始 VA）。
    pub(crate) mmap_anon_cursor : VirtAddr,
    /// 文件 `mmap` bump 指针（与匿名区分离，避免交错碎片）。
    pub(crate) mmap_file_cursor : VirtAddr,
}

impl Sv39AddressSpace {
    /// 分配并清零根页表帧；依赖帧分配器与 [`table_mut`] 的物理访问假设。
    pub(crate) fn new() -> MmResult<Self> {
        let root = alloc_table_frame_zeroed()?;
        Ok(Self { root,
                  user_brk_start : VirtAddr(0),
                  user_brk_current_end : VirtAddr(0),
                  user_brk_max : VirtAddr(0),
                  mmap_anon_cursor : VirtAddr(0),
                  mmap_file_cursor : VirtAddr(0) })
    }

    /// ELF 装载完成后初始化用户堆与匿名映射区游标（须在泄漏页表对象前调用一次）。
    pub(crate) fn init_user_layout(&mut self,
                                   brk_start : VirtAddr,
                                   brk_current_end : VirtAddr,
                                   brk_max : VirtAddr,
                                   mmap_anon_cursor : VirtAddr) {
        self.user_brk_start = brk_start;
        self.user_brk_current_end = brk_current_end;
        self.user_brk_max = brk_max;
        self.mmap_anon_cursor = mmap_anon_cursor;
        self.mmap_file_cursor = mmap_anon_cursor;
    }

    #[inline]
    fn walk_create(&mut self, vpn : VirtPageNum) -> MmResult<(&'static mut Sv39Pte, usize)> {
        let idx = vpn_indexes(vpn);
        let mut ppn = self.root;

        for level in (0..SV39_LEVELS).rev() {
            let table = unsafe { table_mut(ppn) };
            let pte = &mut table[idx[level]];
            let flags = pte.flags();

            if level == 0 {
                return Ok((pte, level));
            }

            if !flags.is_valid() {
                let child = alloc_table_frame_zeroed()?;
                pte.set(child, Sv39PteFlags::V);
            } else if flags.is_leaf() {
                return Err(MmError::AlreadyMapped);
            }

            ppn = pte.ppn();
        }

        Err(MmError::InvalidAddress)
    }

    #[inline]
    fn walk_find(&self, vpn : VirtPageNum) -> MmResult<Option<(&'static mut Sv39Pte, usize)>> {
        let idx = vpn_indexes(vpn);
        let mut ppn = self.root;

        for level in (0..SV39_LEVELS).rev() {
            let table = unsafe { table_mut(ppn) };
            let pte = &mut table[idx[level]];
            let flags = pte.flags();

            if !flags.is_valid() {
                return Ok(None);
            }
            if level == 0 || flags.is_leaf() {
                return Ok(Some((pte, level)));
            }
            ppn = pte.ppn();
        }
        Ok(None)
    }

    /// 创建独立的地址空间副本：递归复制三级页表树。
    ///
    /// - 用户页（PTE 中 `U` 位置位）：分配新物理帧，逐字节复制数据。
    /// - 内核恒等映射页（无 `U`）：共享原始 PPN，不复制数据帧。
    /// - 中间页表帧：分配新帧，仅设 `V` 标志（非叶子）。
    pub fn fork(&self) -> MmResult<Sv39AddressSpace> {
        log::trace!("[mm-fork] Sv39AddressSpace::fork begin root_ppn={}",
                    self.root.0);
        let child_root = alloc_table_frame_zeroed()?;
        // SAFETY: 刚分配并清零的帧作为子地址空间根页表。
        unsafe {
            fork_table(self.root, child_root, SV39_LEVELS - 1)?;
        }
        log::trace!("[mm-fork] Sv39AddressSpace::fork done child_root={}",
                    child_root.0);
        Ok(Sv39AddressSpace { root : child_root,
                              user_brk_start : self.user_brk_start,
                              user_brk_current_end : self.user_brk_current_end,
                              user_brk_max : self.user_brk_max,
                              mmap_anon_cursor : self.mmap_anon_cursor,
                              mmap_file_cursor : self.mmap_file_cursor })
    }

    /// 递归释放所有用户页帧及页表帧，不触碰内核恒等映射。
    ///
    /// 调用后本地址空间不再可用。
    pub fn destroy(&mut self) {
        if self.root.0 == 0 {
            return;
        }
        unsafe {
            destroy_table(self.root, SV39_LEVELS - 1);
        }
        self.root = PhysPageNum(0);
    }
}

/// 递归销毁页表树：释放 U 标志的叶子页对应的物理帧，递归释放子页表帧。
///
/// # Safety
/// 调用方确保 `ppn` 指向有效的 4 KiB 页表帧。
unsafe fn destroy_table(ppn : PhysPageNum, level : usize) {
    let table = unsafe { table_mut(ppn) };
    for pte in table.iter() {
        let flags = pte.flags();
        if !flags.is_valid() {
            continue;
        }
        let child_ppn = pte.ppn();

        if flags.is_leaf() &&
           flags.to_page_perm()
                .user()
        {
            let _ = frame_dealloc_result(child_ppn);
        } else if level > 0 {
            unsafe {
                destroy_table(child_ppn, level - 1);
            }
        }
    }
    let _ = frame_dealloc_result(ppn);
}

/// 递归复制一级页表：遍历 `parent_ppn` 对应的 Sv39 页表项，将
/// 映射关系同步到 `child_ppn` 对应的已清零子表中。
///
/// # Safety
///
/// 调用方确保 `parent_ppn` 和 `child_ppn` 均指向有效的 4 KiB 页表帧，
/// 且 `child_ppn` 内容已清零。`level`
/// 为当前层级：2=根（VPN[2]），1=中间（VPN[1]），0=叶子（VPN[0]）。
unsafe fn fork_table(parent_ppn : PhysPageNum,
                     child_ppn : PhysPageNum,
                     level : usize)
                     -> MmResult<()> {
    let parent_table = unsafe { table_mut(parent_ppn) };
    let child_table = unsafe { table_mut(child_ppn) };

    for (i, pte) in parent_table.iter()
                                .enumerate()
    {
        let flags = pte.flags();
        if !flags.is_valid() {
            continue;
        }
        let ppn = pte.ppn();

        if flags.is_leaf() {
            let perm = flags.to_page_perm();
            if perm.user() {
                let new_ppn = frame_alloc_result().map_err(MmError::from)?;
                let src = ppn.0 * PAGE_SIZE;
                let dst = new_ppn.0 * PAGE_SIZE;
                unsafe {
                    core::ptr::copy_nonoverlapping(src as *const u8,
                                                   dst as *mut u8,
                                                   PAGE_SIZE);
                }
                child_table[i].set(new_ppn, flags);
            } else {
                child_table[i].set(ppn, flags);
            }
        } else if level > 0 {
            let child_sub = alloc_table_frame_zeroed()?;
            child_table[i].set(child_sub, Sv39PteFlags::V);
            unsafe {
                fork_table(ppn, child_sub, level - 1)?;
            }
        }
    }
    Ok(())
}

impl AddressSpaceOps for Sv39AddressSpace {
    fn satp_value(&self) -> usize { (8usize << 60) | (self.root.0 & ((1usize << 44) - 1)) }

    fn map_page_to_ppn(&mut self,
                       vpn : VirtPageNum,
                       ppn : PhysPageNum,
                       perm : PagePerm)
                       -> MmResult<()> {
        let (pte, _level) = self.walk_create(vpn)?;
        if pte.flags()
              .is_valid()
        {
            return Err(MmError::AlreadyMapped);
        }
        pte.set(ppn, Sv39PteFlags::from_perm(perm));
        Ok(())
    }

    fn unmap_page_to_ppn(&mut self, vpn : VirtPageNum) -> MmResult<Option<PhysPageNum>> {
        let Some((pte, _level)) = self.walk_find(vpn)? else {
            return Ok(None);
        };
        if !pte.flags()
               .is_leaf()
        {
            return Ok(None);
        }
        let old = pte.ppn();
        pte.clear();
        Ok(Some(old))
    }

    fn protect_page(&mut self, vpn : VirtPageNum, perm : PagePerm) -> MmResult<()> {
        let Some((pte, _level)) = self.walk_find(vpn)? else {
            return Err(MmError::NotMapped);
        };
        if !pte.flags()
               .is_leaf()
        {
            return Err(MmError::NotMapped);
        }
        let ppn = pte.ppn();
        pte.set(ppn, Sv39PteFlags::from_perm(perm));
        Ok(())
    }

    fn translate_addr(&self, va : VirtAddr) -> MmResult<Option<PhysAddr>> {
        let vpn = va.floor_page();
        let off = va.page_offset();
        let Some((pte, level)) = self.walk_find(vpn)? else {
            return Ok(None);
        };
        if !pte.flags()
               .is_leaf()
        {
            return Ok(None);
        }
        if level != 0 {
            return Err(MmError::Unsupported);
        }
        Ok(Some(PhysAddr(pte.ppn().0 * PAGE_SIZE + off)))
    }

    fn leaf_page_perm(&self, vpn : VirtPageNum) -> MmResult<Option<PagePerm>> {
        let Some((pte, level)) = self.walk_find(vpn)? else {
            return Ok(None);
        };
        if level != 0 ||
           !pte.flags()
               .is_leaf()
        {
            return Ok(None);
        }
        Ok(Some(pte.flags()
                   .to_page_perm()))
    }

    fn fork(&self) -> MmResult<Self> { Sv39AddressSpace::fork(self) }
}

impl Drop for Sv39AddressSpace {
    fn drop(&mut self) {
        self.destroy();
    }
}

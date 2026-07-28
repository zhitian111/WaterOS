//! LoongArch64 三级页表与 **4 KiB 叶子页** 实现；仅本 crate
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
//! 映射时预先置 PTE **D** 位（脏位），避免依赖硬件写时置位触发页故障（早期
//! bring-up 策略）。

extern crate alloc;

use alloc::{boxed::Box, vec::Vec};

use api_v0::addr::{PhysAddr, PhysPageNum, VirtAddr, VirtPageNum, PAGE_SIZE};
use api_v0::address_space::AddressSpaceOps;
use api_v0::error::{MmError, MmResult};
use api_v0::mmap::{DemandPageLoader, PageFaultAccess};
use api_v0::perm::PagePerm;

use frame_alloctor::{frame_alloc_result, frame_dealloc_result, frame_inc_ref, frame_ref_count};

/// LoongArch64 PTE 标志位。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LoongArch64PteFlags(usize);

impl LoongArch64PteFlags {
    const V : Self = Self(1 << 0); // Valid
    const D : Self = Self(1 << 1); // Dirty
                                   // bits [2:3] = PLV (privilege level)
                                   // bits [4:5] = MAT (memory access type)
    const P : Self = Self(1 << 7); // Present (physical page exists)
    const W : Self = Self(1 << 8); // Writable
    const COW : Self = Self(1 << 9);
    const COW_WAS_WRITABLE : Self = Self(1 << 10);
    const NR : Self = Self(1 << 61); // Not Readable
    const NX : Self = Self(1 << 62); // Not Executable
    const RPLV : Self = Self(1 << 63); // Restricted PLV, reserved for future

    /// PLV = 3（用户态可访问）。
    const PLV_USER : usize = 3usize << 2;
    /// MAT = 1（Coherent Cached，一致可缓存）。
    const MAT_CACHED : usize = 1usize << 4;

    const PLV_MASK : usize = 3usize << 2;

    #[inline]
    const fn bits(self) -> usize { self.0 }

    #[inline]
    const fn is_valid(self) -> bool { (self.0 & Self::V.0) != 0 }

    #[inline]
    const fn is_present(self) -> bool { (self.0 & Self::P.0) != 0 }

    /// 判断是否为叶子页表项：LoongArch 普通页 PTE 使用 P 位表示物理页存在；
    /// 非叶目录项是纯物理地址，不能设置 V/P 等低位标志，否则硬件 LDDIR 会把
    /// 这些低位当作下一级表基址的一部分。
    #[inline]
    const fn is_leaf(self) -> bool { self.is_valid() && self.is_present() }

    #[inline]
    const fn writable(self) -> bool { (self.0 & Self::W.0) != 0 }

    #[inline]
    const fn cow(self) -> bool { (self.0 & Self::COW.0) != 0 }

    #[inline]
    const fn cow_was_writable(self) -> bool { (self.0 & Self::COW_WAS_WRITABLE.0) != 0 }

    #[inline]
    fn prepare_cow(self) -> Self {
        let mut f = self;
        // LoongArch 的 W/P 是软件页表遍历辅助位，不会进入 TLB 权限检查；
        // 清 D 位让用户 store 触发 PME，再由 trap 路径完成写时复制。
        f.0 &= !(Self::W.0 | Self::D.0);
        f.0 |= Self::COW.0 | Self::COW_WAS_WRITABLE.0;
        f
    }

    #[inline]
    fn clear_cow(self) -> Self {
        Self(self.0 & !(Self::COW.0 | Self::COW_WAS_WRITABLE.0))
    }

    #[inline]
    fn restore_cow_writable(self) -> Self {
        let mut f = self.clear_cow();
        f.0 |= Self::W.0 | Self::D.0;
        f
    }

    /// 从 [`PagePerm`] 构造 PTE 标志：V=1, D=1 (pre-fault), MAT=CoherentCached,
    /// PLV 由 `perm.user()` 决定。
    #[inline]
    fn from_perm(perm : PagePerm) -> Self {
        let mut f = Self::V; // always valid
        f.0 |= Self::D.0; // pre-set dirty (eager bring-up)
        f.0 |= Self::P.0; // page is present
        f.0 |= Self::MAT_CACHED;
        if perm.user() {
            f.0 |= Self::PLV_USER; // PLV = 3
        }
        // else PLV stays 0 (kernel-only)
        if perm.writable() {
            f.0 |= Self::W.0;
        }
        if !perm.readable() {
            f.0 |= Self::NR.0;
        }
        if !perm.executable() {
            f.0 |= Self::NX.0;
        }
        f
    }

    #[inline]
    fn to_page_perm(self) -> PagePerm {
        let mut p = PagePerm::empty();
        if (self.0 & Self::NR.0) == 0 {
            p = p | PagePerm::R;
        }
        if (self.0 & Self::W.0) != 0 {
            p = p | PagePerm::W;
        }
        if (self.0 & Self::NX.0) == 0 {
            p = p | PagePerm::X;
        }
        if (self.0 & Self::PLV_MASK) == Self::PLV_USER {
            p = p | PagePerm::U;
        }
        p
    }
}

/// LoongArch64 PTE：64 位，低 12 位为标志，高 52 位为 PPN。
#[repr(transparent)]
#[derive(Clone, Copy)]
struct LoongArch64Pte(usize);

impl LoongArch64Pte {
    #[allow(dead_code)]
    #[inline]
    const fn zero() -> Self { Self(0) }

    #[inline]
    fn flags(self) -> LoongArch64PteFlags {
        LoongArch64PteFlags((self.0 & 0x7FF) |
                             (self.0 & (LoongArch64PteFlags::NR.0 |
                                        LoongArch64PteFlags::NX.0 |
                                        LoongArch64PteFlags::RPLV.0)))
    }

    #[inline]
    fn ppn(self) -> PhysPageNum { PhysPageNum((self.0 >> 12) & ((1usize << (48 - 12)) - 1)) }

    #[inline]
    fn set(&mut self, ppn : PhysPageNum, flags : LoongArch64PteFlags) {
        self.0 = (ppn.0 << 12) | flags.bits();
    }

    #[inline]
    fn set_table(&mut self, ppn : PhysPageNum) { self.0 = ppn.0 << 12; }

    #[inline]
    fn clear(&mut self) { self.0 = 0; }
}

const LOONGARCH64_LEVELS : usize = 3;
const LOONGARCH64_ENTRIES : usize = 512;
const VPN_INDEX_BITS : usize = 9;
pub(crate) const USER_VA_LIMIT : usize = 0x0000_0080_0000_0000;
const KERNEL_IDENTITY_BASE : usize = 0x9000_0000;

/// 将 VPN 拆分为三级索引：`[VPN[0], VPN[1], VPN[2]]`，
/// 与 Sv39 `vpn_indexes` 语义相同。
#[inline]
fn vpn_indexes(vpn : VirtPageNum) -> [usize; 3] {
    let v = vpn.0;
    [(v >> 0) & 0x1FF, (v >> 9) & 0x1FF, (v >> 18) & 0x1FF]
}

/// # Safety
///
/// 调用方保证 `ppn` 指向已映射且 **4 KiB 对齐** 的页表存储。
#[inline]
unsafe fn table_mut(ppn : PhysPageNum) -> &'static mut [LoongArch64Pte; LOONGARCH64_ENTRIES] {
    let pa = ppn.0 * PAGE_SIZE;
    unsafe { &mut *(pa as *mut [LoongArch64Pte; LOONGARCH64_ENTRIES]) }
}

/// 将已分配的用户数据帧清零，避免匿名页/栈页复用时暴露旧内容。
#[inline]
pub(crate) fn zero_phys_page(ppn : PhysPageNum) {
    let pa = ppn.0 * PAGE_SIZE;
    unsafe {
        core::ptr::write_bytes(pa as *mut u8, 0, PAGE_SIZE);
    }
}

#[inline]
fn alloc_table_frame_zeroed() -> MmResult<PhysPageNum> {
    let ppn = frame_alloc_result().map_err(MmError::from)?;
    zero_phys_page(ppn);
    Ok(ppn)
}

/// LoongArch64 根页表与 walk 状态；所有映射均为 **4 KiB 叶子**。
pub struct LoongArch64AddressSpace {
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
    /// mmap arena 起点，用于 first-fit 复用 `munmap` 后的空洞。
    pub(crate) mmap_base : VirtAddr,
    /// 用户栈保留区，可由合法读/写缺页按需补页。
    pub(crate) user_stack_bottom : VirtAddr,
    pub(crate) user_stack_top : VirtAddr,
    pub(crate) lazy_file_vmas : Vec<LazyFileVma>,
    pub(crate) shared_anon_vmas : Vec<SharedAnonVma>,
}

// The address space is accessed through MultiprocessorSafeCell.  The lock
// serializes the non-Send lazy-loader state as well as page-table mutation.
unsafe impl Send for LoongArch64AddressSpace {}
unsafe impl Sync for LoongArch64AddressSpace {}

// 本结构代码由AI完成
pub(crate) struct LazyFileVma {
    pub start : VirtAddr,
    pub end : VirtAddr,
    pub perm : PagePerm,
    pub file_offset : usize,
    pub file_size : usize,
    pub loader : Box<dyn DemandPageLoader>,
}

impl LazyFileVma {
    fn duplicate(&self) -> MmResult<Self> {
        Ok(Self { start : self.start,
                  end : self.end,
                  perm : self.perm,
                  file_offset : self.file_offset,
                  file_size : self.file_size,
                  loader : self.loader.duplicate_box()? })
    }

    fn contains_page(&self, page : VirtAddr) -> bool { page.0 >= self.start.0 && page.0 < self.end.0 }

    fn overlaps(&self, start : VirtAddr, end : VirtAddr) -> bool {
        start.0 < self.end.0 && end.0 > self.start.0
    }
}

// 本结构代码由AI完成
#[derive(Clone, Copy)]
pub(crate) struct SharedAnonVma {
    pub start : VirtAddr,
    pub end : VirtAddr,
}

impl SharedAnonVma {
    fn contains_page(&self, page : VirtAddr) -> bool { page.0 >= self.start.0 && page.0 < self.end.0 }

    fn overlaps(&self, start : VirtAddr, end : VirtAddr) -> bool {
        start.0 < self.end.0 && end.0 > self.start.0
    }
}

impl LoongArch64AddressSpace {
    /// 分配并清零根页表帧；依赖帧分配器与 [`table_mut`] 的物理访问假设。
    pub(crate) fn new() -> MmResult<Self> {
        let root = alloc_table_frame_zeroed()?;
        Ok(Self { root,
                  user_brk_start : VirtAddr(0),
                  user_brk_current_end : VirtAddr(0),
                  user_brk_max : VirtAddr(0),
                  mmap_anon_cursor : VirtAddr(0),
                  mmap_file_cursor : VirtAddr(0),
                  mmap_base : VirtAddr(0),
                  user_stack_bottom : VirtAddr(0),
                  user_stack_top : VirtAddr(0),
                  lazy_file_vmas : Vec::new(),
                  shared_anon_vmas : Vec::new() })
    }

    /// ELF 装载完成后初始化用户堆与匿名映射区游标（须在泄漏页表对象前调用一次）。
    pub(crate) fn init_user_layout(&mut self,
                                   brk_start : VirtAddr,
                                   brk_current_end : VirtAddr,
                                   brk_max : VirtAddr,
                                   mmap_anon_cursor : VirtAddr,
                                   stack_bottom : VirtAddr,
                                   stack_top : VirtAddr) {
        self.user_brk_start = brk_start;
        self.user_brk_current_end = brk_current_end;
        self.user_brk_max = brk_max;
        self.mmap_anon_cursor = mmap_anon_cursor;
        self.mmap_file_cursor = mmap_anon_cursor;
        self.mmap_base = mmap_anon_cursor;
        self.user_stack_bottom = stack_bottom;
        self.user_stack_top = stack_top;
    }

    pub(crate) fn range_overlaps_stack(&self, start : VirtAddr, end : VirtAddr) -> bool {
        self.user_stack_bottom.0 < self.user_stack_top.0 &&
        start.0 < self.user_stack_top.0 &&
        end.0 > self.user_stack_bottom.0
    }

    pub(crate) fn range_overlaps_kernel_reserved(&self, start : VirtAddr, end : VirtAddr) -> bool {
        let kernel_end = crate::kernel_global::phys_ram_end_exclusive();
        start.0 < kernel_end && end.0 > KERNEL_IDENTITY_BASE
    }

    pub(crate) fn validate_user_mapping_range(&self,
                                              start : VirtAddr,
                                              end : VirtAddr)
                                              -> MmResult<()> {
        if start.0 >= end.0 || end.0 > USER_VA_LIMIT {
            return Err(MmError::InvalidAddress);
        }
        if self.range_overlaps_stack(start, end) ||
           self.range_overlaps_kernel_reserved(start, end)
        {
            return Err(MmError::InvalidAddress);
        }
        Ok(())
    }

    pub(crate) fn lazy_vma_overlaps(&self, start : VirtAddr, end : VirtAddr) -> bool {
        self.lazy_file_vmas.iter()
                           .any(|vma| vma.overlaps(start, end))
    }

    #[allow(dead_code)]
    pub(crate) fn lazy_vma_contains(&self, page : VirtAddr) -> bool {
        self.lazy_file_vmas.iter()
                           .any(|vma| vma.contains_page(page))
    }

    #[allow(dead_code)]
    pub(crate) fn merge_lazy_file_vma_perm(&mut self,
                                           start : VirtAddr,
                                           end : VirtAddr,
                                           perm : PagePerm)
                                           -> MmResult<()> {
        let mut next = Vec::new();
        for vma in self.lazy_file_vmas.drain(..) {
            if !vma.overlaps(start, end) {
                next.push(vma);
                continue;
            }
            if start.0 > vma.start.0 {
                next.push(LazyFileVma { start : vma.start,
                                        end : start,
                                        perm : vma.perm,
                                        file_offset : vma.file_offset,
                                        file_size : vma.file_size,
                                        loader : vma.loader
                                                    .duplicate_box()? });
            }
            let mid_start = VirtAddr(core::cmp::max(start.0, vma.start.0));
            let mid_end = VirtAddr(core::cmp::min(end.0, vma.end.0));
            next.push(LazyFileVma { start : mid_start,
                                    end : mid_end,
                                    perm : vma.perm | perm,
                                    file_offset : vma.file_offset + (mid_start.0 - vma.start.0),
                                    file_size : vma.file_size,
                                    loader : vma.loader
                                                .duplicate_box()? });
            if end.0 < vma.end.0 {
                next.push(LazyFileVma { start : end,
                                        end : vma.end,
                                        perm : vma.perm,
                                        file_offset : vma.file_offset + (end.0 - vma.start.0),
                                        file_size : vma.file_size,
                                        loader : vma.loader });
            }
        }
        self.lazy_file_vmas = next;
        Ok(())
    }

    pub(crate) fn shared_anon_vma_overlaps(&self, start : VirtAddr, end : VirtAddr) -> bool {
        self.shared_anon_vmas.iter()
                             .any(|vma| vma.overlaps(start, end))
    }

    pub(crate) fn register_shared_anon_vma(&mut self, start : VirtAddr, end : VirtAddr) {
        self.shared_anon_vmas.push(SharedAnonVma { start, end });
    }

    pub(crate) fn remove_shared_anon_vmas(&mut self, start : VirtAddr, end : VirtAddr) {
        let mut next = Vec::new();
        for vma in self.shared_anon_vmas.drain(..) {
            if !vma.overlaps(start, end) {
                next.push(vma);
                continue;
            }
            if start.0 > vma.start.0 {
                next.push(SharedAnonVma { start : vma.start,
                                          end : start });
            }
            if end.0 < vma.end.0 {
                next.push(SharedAnonVma { start : end,
                                          end : vma.end });
            }
        }
        self.shared_anon_vmas = next;
    }

    pub(crate) fn find_free_mmap_base_considering_vmas(&self,
                                                       cursor : VirtAddr,
                                                       len : usize)
                                                       -> MmResult<VirtAddr> {
        const MAX_SEARCH_PAGES : usize = 1 << 20;
        if len == 0 {
            return Err(MmError::InvalidAddress);
        }
        let n_pages = len.checked_add(PAGE_SIZE - 1)
                         .ok_or(MmError::InvalidAddress)? /
                      PAGE_SIZE;
        let _hint = cursor;
        let brk_guard = self.user_brk_current_end.ceil_page()
                                                 .start_addr();
        let mut base = VirtAddr(core::cmp::max(self.mmap_base.0, brk_guard.0)).ceil_page()
                                                                              .start_addr();
        let mut skipped = 0usize;
        loop {
            if skipped > MAX_SEARCH_PAGES {
                return Err(MmError::InvalidAddress);
            }
            let end = VirtAddr(base.0
                                   .checked_add(n_pages.checked_mul(PAGE_SIZE)
                                                          .ok_or(MmError::InvalidAddress)?)
                                   .ok_or(MmError::InvalidAddress)?);
            if end.0 <= USER_VA_LIMIT &&
               !self.range_overlaps_stack(base, end) &&
               !self.range_overlaps_kernel_reserved(base, end) &&
               !self.lazy_vma_overlaps(base, end) &&
               !self.shared_anon_vma_overlaps(base, end)
            {
                let mut free = true;
                for i in 0..n_pages {
                    let va = VirtAddr(base.0
                                          .checked_add(i.checked_mul(PAGE_SIZE)
                                                        .ok_or(MmError::InvalidAddress)?)
                                          .ok_or(MmError::InvalidAddress)?);
                    if self.translate_addr(va)?
                           .is_some()
                    {
                        free = false;
                        break;
                    }
                }
                if free {
                    return Ok(base);
                }
            }
            skipped += 1;
            base = VirtAddr(base.0
                                .checked_add(PAGE_SIZE)
                                .ok_or(MmError::InvalidAddress)?);
        }
    }

    pub(crate) fn register_lazy_file_vma(&mut self,
                                         start : VirtAddr,
                                         end : VirtAddr,
                                         perm : PagePerm,
                                         file_offset : usize,
                                         file_size : usize,
                                         loader : Box<dyn DemandPageLoader>)
                                         -> MmResult<()> {
        self.validate_user_mapping_range(start, end)?;
        if self.lazy_vma_overlaps(start, end) {
            return Err(MmError::InvalidAddress);
        }
        self.ensure_lazy_refill_paths(start, end)?;
        self.lazy_file_vmas.push(LazyFileVma { start,
                                               end,
                                               perm,
                                               file_offset,
                                               file_size,
                                               loader });
        Ok(())
    }

    /// Allocate the directory levels needed by the hardware refill walker.
    ///
    /// Linux points every empty directory slot at shared invalid lower-level
    /// tables. WaterOS uses zero-filled directories instead, so lazy VMAs must
    /// materialize their directory path while keeping the leaf PTE invalid.
    fn ensure_lazy_refill_paths(&mut self, start : VirtAddr, end : VirtAddr) -> MmResult<()> {
        const LEAF_TABLE_SPAN : usize = PAGE_SIZE * LOONGARCH64_ENTRIES;

        let last = end.0.checked_sub(1)
                        .ok_or(MmError::InvalidAddress)?;
        let mut address = start.floor_page()
                               .start_addr()
                               .0;
        loop {
            let _ = self.walk_create(VirtAddr(address).floor_page())?;
            let next = address.checked_div(LEAF_TABLE_SPAN)
                              .and_then(|span| span.checked_add(1))
                              .and_then(|span| span.checked_mul(LEAF_TABLE_SPAN))
                              .ok_or(MmError::InvalidAddress)?;
            if next > last {
                return Ok(());
            }
            address = next;
        }
    }

    pub(crate) fn remove_lazy_file_vmas(&mut self, start : VirtAddr, end : VirtAddr) -> MmResult<()> {
        let mut next = Vec::new();
        for vma in self.lazy_file_vmas.drain(..) {
            if !vma.overlaps(start, end) {
                next.push(vma);
                continue;
            }
            if start.0 > vma.start.0 {
                next.push(LazyFileVma { start : vma.start,
                                        end : start,
                                        perm : vma.perm,
                                        file_offset : vma.file_offset,
                                        file_size : vma.file_size,
                                        loader : vma.loader.duplicate_box()? });
            }
            if end.0 < vma.end.0 {
                let delta = end.0.saturating_sub(vma.start.0);
                next.push(LazyFileVma { start : end,
                                        end : vma.end,
                                        perm : vma.perm,
                                        file_offset : vma.file_offset.saturating_add(delta),
                                        file_size : vma.file_size,
                                        loader : vma.loader });
            }
        }
        self.lazy_file_vmas = next;
        Ok(())
    }

    pub(crate) fn protect_lazy_file_vmas(&mut self,
                                         start : VirtAddr,
                                         end : VirtAddr,
                                         perm : PagePerm)
                                         -> MmResult<()> {
        let mut next = Vec::new();
        for vma in self.lazy_file_vmas.drain(..) {
            if !vma.overlaps(start, end) {
                next.push(vma);
                continue;
            }
            if start.0 > vma.start.0 {
                next.push(LazyFileVma { start : vma.start,
                                        end : start,
                                        perm : vma.perm,
                                        file_offset : vma.file_offset,
                                        file_size : vma.file_size,
                                        loader : vma.loader.duplicate_box()? });
            }
            let mid_start = VirtAddr(core::cmp::max(start.0, vma.start.0));
            let mid_end = VirtAddr(core::cmp::min(end.0, vma.end.0));
            next.push(LazyFileVma { start : mid_start,
                                    end : mid_end,
                                    perm,
                                    file_offset : vma.file_offset + (mid_start.0 - vma.start.0),
                                    file_size : vma.file_size,
                                    loader : vma.loader.duplicate_box()? });
            if end.0 < vma.end.0 {
                next.push(LazyFileVma { start : end,
                                        end : vma.end,
                                        perm : vma.perm,
                                        file_offset : vma.file_offset + (end.0 - vma.start.0),
                                        file_size : vma.file_size,
                                        loader : vma.loader });
            }
        }
        self.lazy_file_vmas = next;
        Ok(())
    }

    /// 沿 VPN 三级索引向下 walk，必要时分配中间页表；返回目标叶子 PTE 槽位。
    #[inline]
    fn walk_create(&mut self, vpn : VirtPageNum) -> MmResult<(&'static mut LoongArch64Pte, usize)> {
        let idx = vpn_indexes(vpn);
        let mut ppn = self.root;

        for level in (0..LOONGARCH64_LEVELS).rev() {
            let table = unsafe { table_mut(ppn) };
            let pte = &mut table[idx[level]];
            let flags = pte.flags();

            if level == 0 {
                return Ok((pte, level));
            }

            if pte.0 == 0 {
                // 目录项为空：分配子表，仅写入 PPN（非叶格式，不设 V/P）
                let child = alloc_table_frame_zeroed()?;
                pte.set_table(child);
            } else if flags.is_leaf() {
                return Err(MmError::AlreadyMapped);
            }

            ppn = pte.ppn();
        }

        Err(MmError::InvalidAddress)
    }

    /// 只读 walk：找到叶子或中途停止。
    #[inline]
    fn walk_find(&self,
                 vpn : VirtPageNum)
                 -> MmResult<Option<(&'static mut LoongArch64Pte, usize)>> {
        let idx = vpn_indexes(vpn);
        let mut ppn = self.root;

        for level in (0..LOONGARCH64_LEVELS).rev() {
            let table = unsafe { table_mut(ppn) };
            let pte = &mut table[idx[level]];
            let flags = pte.flags();

            if pte.0 == 0 {
                return Ok(None);
            }
            if level == 0 || flags.is_leaf() {
                return Ok(Some((pte, level)));
            }
            ppn = pte.ppn();
        }
        Ok(None)
    }

    /// 创建 COW 地址空间副本：递归复制三级页表树。
    ///
    /// - 用户页（PTE 中 `PLV == 3`）：共享原物理帧，写页清 `W` 并标记 COW。
    /// - 内核恒等映射页（PLV != 3）：共享原始 PPN，不复制数据帧。
    /// - 含用户映射的中间页表帧：为子地址空间复制页表结构。
    // 本方法代码由AI完成
    pub fn fork_cow(&mut self) -> MmResult<LoongArch64AddressSpace> {
        log::trace!("[mm-fork] LoongArch64AddressSpace::fork begin root_ppn={}",
                    self.root.0);
        let child_root = alloc_table_frame_zeroed()?;
        // SAFETY: 刚分配并清零的帧作为子地址空间根页表。
        if let Err(err) = unsafe {
            fork_table(self.root,
                       child_root,
                       LOONGARCH64_LEVELS - 1,
                       0,
                       &self.shared_anon_vmas)
        } {
            unsafe {
                destroy_table(child_root, LOONGARCH64_LEVELS - 1, 0, &self.shared_anon_vmas);
            }
            return Err(err);
        }
        platform::arch::paging::flush_address_space_translations();
        log::trace!("[mm-fork] LoongArch64AddressSpace::fork done child_root={}",
                    child_root.0);
        Ok(LoongArch64AddressSpace { root : child_root,
                                     user_brk_start : self.user_brk_start,
                                     user_brk_current_end : self.user_brk_current_end,
                                     user_brk_max : self.user_brk_max,
                              mmap_anon_cursor : self.mmap_anon_cursor,
                              mmap_file_cursor : self.mmap_file_cursor,
                              mmap_base : self.mmap_base,
                              user_stack_bottom : self.user_stack_bottom,
                              user_stack_top : self.user_stack_top,
                              lazy_file_vmas : self.lazy_file_vmas.iter()
                                                                         .map(LazyFileVma::duplicate)
                                                                         .collect::<MmResult<Vec<_>>>()?,
                              shared_anon_vmas : self.shared_anon_vmas.clone() })
    }

    // 本方法代码由AI完成
    fn handle_cow_page(&mut self, vpn : VirtPageNum) -> MmResult<bool> {
        let Some((pte, level)) = self.walk_find(vpn)? else {
            return Ok(false);
        };
        let flags = pte.flags();
        if level != 0 || !flags.is_leaf() || !flags.cow() || !flags.cow_was_writable() {
            return Ok(false);
        }
        let old_ppn = pte.ppn();
        let new_flags = flags.restore_cow_writable();
        if frame_ref_count(old_ppn).map_err(MmError::from)? <= 1 {
            pte.set(old_ppn, new_flags);
            platform::arch::paging::flush_address_space_translations();
            return Ok(true);
        }

        // 引用计数 > 1：复制整页并切换 PTE 指向新帧
        let new_ppn = frame_alloc_result().map_err(MmError::from)?;
        let src = old_ppn.0 * PAGE_SIZE;
        let dst = new_ppn.0 * PAGE_SIZE;
        unsafe {
            core::ptr::copy_nonoverlapping(src as *const u8, dst as *mut u8, PAGE_SIZE);
        }
        frame_dealloc_result(old_ppn).map_err(MmError::from)?;
        pte.set(new_ppn, new_flags);
        platform::arch::paging::flush_address_space_translations();
        Ok(true)
    }

    // 本方法代码由AI完成
    pub fn handle_cow_fault(&mut self, fault_addr : VirtAddr) -> MmResult<bool> {
        self.handle_cow_page(fault_addr.floor_page())
    }

    // 本方法代码由AI完成
    pub fn handle_lazy_page_fault<A>(&mut self,
                                     allocator : &mut A,
                                     fault_addr : VirtAddr,
                                     access : PageFaultAccess)
                                     -> MmResult<bool>
        where A : api_v0::frame_allocator::PhysicalFrameAllocator<FrameId = PhysPageNum>
    {
        let page = fault_addr.floor_page()
                             .start_addr();
        let Some(index) = self.lazy_file_vmas.iter()
                                             .position(|vma| vma.contains_page(page))
        else {
            return Ok(false);
        };
        let perm = self.lazy_file_vmas[index].perm;
        let allowed = match access {
            PageFaultAccess::Read => perm.readable(),
            PageFaultAccess::Write => perm.writable(),
            PageFaultAccess::Execute => perm.executable(),
        };
        if !allowed || !perm.user() {
            return Ok(false);
        }
        if self.translate_addr(page)?
               .is_some()
        {
            return Ok(true);
        }
        let ppn = allocator.alloc_frame()?;
        let pa = ppn.0 * PAGE_SIZE;
        let dst = unsafe { core::slice::from_raw_parts_mut(pa as *mut u8, PAGE_SIZE) };
        dst.fill(0);
        let file_offset = {
            let vma = &self.lazy_file_vmas[index];
            vma.file_offset + (page.0 - vma.start.0)
        };
        if let Err(e) = self.lazy_file_vmas[index].loader.load_page(file_offset, dst) {
            let _ = allocator.dealloc_frame(ppn);
            return Err(e);
        }
        if let Err(e) = self.map_page_to_ppn(page.floor_page(), ppn, perm) {
            let _ = allocator.dealloc_frame(ppn);
            return Err(e);
        }
        platform::arch::paging::flush_address_space_translations();
        Ok(true)
    }

    // 本方法代码由AI完成
    pub fn ensure_private_for_write(&mut self, vpn : VirtPageNum) -> MmResult<bool> {
        if self.handle_cow_page(vpn)? {
            return Ok(true);
        }
        let Some((pte, level)) = self.walk_find(vpn)? else {
            return Ok(false);
        };
        let flags = pte.flags();
        if level != 0 ||
           !flags.is_leaf() ||
           !flags.to_page_perm()
                 .user()
        {
            return Ok(false);
        }
        let old_ppn = pte.ppn();
        if frame_ref_count(old_ppn).map_err(MmError::from)? <= 1 {
            return Ok(true);
        }
        let new_ppn = frame_alloc_result().map_err(MmError::from)?;
        let src = old_ppn.0 * PAGE_SIZE;
        let dst = new_ppn.0 * PAGE_SIZE;
        unsafe {
            core::ptr::copy_nonoverlapping(src as *const u8, dst as *mut u8, PAGE_SIZE);
        }
        frame_dealloc_result(old_ppn).map_err(MmError::from)?;
        pte.set(new_ppn, flags.clear_cow());
        platform::arch::paging::flush_address_space_translations();
        Ok(true)
    }

    /// 递归释放所有用户页帧及页表帧，不触碰内核恒等映射。
    ///
    /// 调用后本地址空间不再可用。
    pub fn destroy(&mut self) {
        if self.root.0 == 0 {
            return;
        }
        unsafe {
            destroy_table(self.root,
                          LOONGARCH64_LEVELS - 1,
                          0,
                          &self.shared_anon_vmas);
        }
        self.root = PhysPageNum(0);
    }
}

/// 递归销毁页表树：释放 PLV==3（用户）的叶子页对应的物理帧，并释放本地址空间拥有的页表帧。
///
/// # Safety
/// 调用方确保 `ppn` 指向有效的 4 KiB 页表帧。
unsafe fn destroy_table(ppn : PhysPageNum,
                        level : usize,
                        vpn_prefix : usize,
                        shared_anon_vmas : &[SharedAnonVma]) {
    let table = unsafe { table_mut(ppn) };
    for i in 0..LOONGARCH64_ENTRIES {
        let pte = table[i];
        let flags = pte.flags();
        if pte.0 == 0 {
            continue;
        }
        let child_ppn = pte.ppn();

        if flags.is_leaf() {
            if flags.to_page_perm()
                    .user()
            {
                let page = VirtPageNum(vpn_prefix | (i << (level * VPN_INDEX_BITS))).start_addr();
                let is_shared_anon = shared_anon_vmas.iter()
                                                     .any(|vma| vma.contains_page(page));
                if !is_shared_anon {
                    let _ = frame_dealloc_result(child_ppn);
                }
            }
        } else if level > 0 {
            let child_prefix = vpn_prefix | (i << (level * VPN_INDEX_BITS));
            unsafe {
                destroy_table(child_ppn, level - 1, child_prefix, shared_anon_vmas);
            }
        }
    }
    let _ = frame_dealloc_result(ppn);
}

/// 递归复制一级页表：遍历 `parent_ppn` 对应的页表项，将
/// 映射关系同步到 `child_ppn` 对应的已清零子表中。
///
/// # Safety
///
/// 调用方确保 `parent_ppn` 和 `child_ppn` 均指向有效的 4 KiB 页表帧，
/// 且 `child_ppn` 内容已清零。`level`
/// 为当前层级：2=根（VPN[2]），1=中间（VPN[1]），0=叶子（VPN[0]）。
unsafe fn fork_table(parent_ppn : PhysPageNum,
                     child_ppn : PhysPageNum,
                     level : usize,
                     vpn_prefix : usize,
                     shared_anon_vmas : &[SharedAnonVma])
                     -> MmResult<()> {
    let parent_table = unsafe { table_mut(parent_ppn) };
    let child_table = unsafe { table_mut(child_ppn) };

    for i in 0..LOONGARCH64_ENTRIES {
        let pte = parent_table[i];
        let flags = pte.flags();
        if pte.0 == 0 {
            continue;
        }
        let ppn = pte.ppn();

        if flags.is_leaf() {
            let perm = flags.to_page_perm();
            if perm.user() {
                let page = VirtPageNum(vpn_prefix | (i << (level * VPN_INDEX_BITS))).start_addr();
                let is_shared_anon = shared_anon_vmas.iter()
                                                     .any(|vma| vma.contains_page(page));
                if !is_shared_anon {
                    frame_inc_ref(ppn).map_err(MmError::from)?;
                }
                let child_flags = if is_shared_anon {
                    flags
                } else if flags.writable() {
                    let cow_flags = flags.prepare_cow();
                    parent_table[i].set(ppn, cow_flags);
                    cow_flags
                } else {
                    flags
                };
                child_table[i].set(ppn, child_flags);
            } else {
                child_table[i].set(ppn, flags);
            }
        } else if level > 0 {
            let child_prefix = vpn_prefix | (i << (level * VPN_INDEX_BITS));
            let child_sub = alloc_table_frame_zeroed()?;
            if let Err(err) =
                unsafe { fork_table(ppn, child_sub, level - 1, child_prefix, shared_anon_vmas) }
            {
                unsafe {
                    destroy_table(child_sub, level - 1, child_prefix, shared_anon_vmas);
                }
                return Err(err);
            }
            child_table[i].set_table(child_sub);
        }
    }
    Ok(())
}

impl AddressSpaceOps for LoongArch64AddressSpace {
    /// LoongArch64 用 CSR.PGDL 存储根页表物理基址（不是 Sv39 的 satp 编码）。
    /// 此方法返回 PGDL 可直接写入值：`root_ppn * PAGE_SIZE`。
    fn satp_value(&self) -> usize { self.root.0 * PAGE_SIZE }

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
        pte.set(ppn,
                LoongArch64PteFlags::from_perm(perm));
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
        pte.set(ppn,
                LoongArch64PteFlags::from_perm(perm));
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

    fn fork(&self) -> MmResult<Self> { Err(MmError::Unsupported) }
}

impl Drop for LoongArch64AddressSpace {
    fn drop(&mut self) {
        self.destroy();
    }
}

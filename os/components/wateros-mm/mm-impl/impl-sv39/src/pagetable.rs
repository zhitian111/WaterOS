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

extern crate alloc;

use alloc::{boxed::Box, sync::Arc, vec::Vec};

use api_v0::addr::{PhysAddr, PhysPageNum, VirtAddr, VirtPageNum, PAGE_SIZE};
use api_v0::address_space::AddressSpaceOps;
use api_v0::error::{MmError, MmResult};
use api_v0::mmap::{DemandPageLoader, DeviceMappingLease, PageFaultAccess};
use api_v0::perm::PagePerm;

use frame_alloctor::{frame_alloc_result, frame_dealloc_result, frame_inc_ref, frame_ref_count};

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
    const COW : Self = Self(1 << 8);
    const COW_WAS_WRITABLE : Self = Self(1 << 9);

    #[inline]
    const fn empty() -> Self { Self(0) }

    #[inline]
    const fn bits(self) -> u16 { self.0 }

    #[inline]
    const fn is_valid(self) -> bool { (self.0 & Self::V.0) != 0 }

    #[inline]
    const fn is_leaf(self) -> bool { (self.0 & (Self::R.0 | Self::W.0 | Self::X.0)) != 0 }

    #[inline]
    const fn writable(self) -> bool { (self.0 & Self::W.0) != 0 }

    #[inline]
    const fn cow(self) -> bool { (self.0 & Self::COW.0) != 0 }

    #[inline]
    const fn cow_was_writable(self) -> bool { (self.0 & Self::COW_WAS_WRITABLE.0) != 0 }

    #[inline]
    fn prepare_cow(self) -> Self {
        let mut f = self;
        f.0 &= !Self::W.0;
        f.0 |= Self::COW.0 | Self::COW_WAS_WRITABLE.0;
        f
    }

    #[inline]
    fn clear_cow(self) -> Self { Self(self.0 & !(Self::COW.0 | Self::COW_WAS_WRITABLE.0)) }

    #[inline]
    fn restore_cow_writable(self) -> Self {
        let mut f = self.clear_cow();
        f.0 |= Self::W.0 | Self::A.0 | Self::D.0;
        f
    }

    /// Sv39 在 level 0 的有效 PTE 即 4 KiB 叶子（含 `PROT_NONE`：V|U 无 R/W/X）；
    /// 更高层级仅 R/W/X 置位时为 superpage 叶子，否则为下一级页表指针。
    #[inline]
    const fn is_leaf_at_level(self, level : usize) -> bool {
        if level == 0 {
            self.is_valid()
        } else {
            self.is_leaf()
        }
    }

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
    #[allow(dead_code)]
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
const VPN_INDEX_BITS : usize = 9;
const SV39_SATP_MODE : usize = 8usize << 60;
pub(crate) const USER_VA_LIMIT : usize = 0x0000_0040_0000_0000;

unsafe extern "C" {
    static __alltraps: u8;
    static __wateros_riscv_restore_user_from_frame: u8;
    static __wateros_riscv_kernel_satp: usize;
    static __wateros_riscv_return_frame: u8;
}

#[inline]
fn page_range_overlaps_addr(start : VirtAddr, end : VirtAddr, addr : usize) -> bool {
    let page_start = VirtAddr(addr).floor_page()
                                   .start_addr()
                                   .0;
    let page_end = page_start + PAGE_SIZE;
    start.0 < page_end && end.0 > page_start
}

#[inline]
fn make_satp(root : PhysPageNum, asid : usize) -> usize {
    SV39_SATP_MODE |
    ((asid & crate::asid::TOKEN_ASID_MASK) << crate::asid::TOKEN_ASID_SHIFT) |
    (root.0 & ((1usize << 44) - 1))
}

#[inline]
fn vpn_indexes(vpn : VirtPageNum) -> [usize; 3] {
    let v = vpn.0;
    [(v >> 0) & 0x1FF,
     (v >> 9) & 0x1FF,
     (v >> 18) & 0x1FF]
}

/// # Safety
///
/// 调用方保证 `ppn` 指向已映射且 **4 KiB 对齐** 的页表存储。
#[inline]
unsafe fn table_mut(ppn : PhysPageNum) -> &'static mut [Sv39Pte; SV39_ENTRIES] {
    let pa = ppn.0 * PAGE_SIZE;
    unsafe { &mut *(pa as *mut [Sv39Pte; SV39_ENTRIES]) }
}

/// 将已分配的用户数据帧清零（匿名 brk/mmap/栈复用帧时避免残留指针）。
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

/// Sv39 根页表与 walk 状态；所有映射均为 **4 KiB 叶子**。
pub struct Sv39AddressSpace {
    root : PhysPageNum,
    asid : u16,
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
    pub(crate) shared_file_vmas : Vec<SharedFileVma>,
    /// 不属于通用帧分配器的外部设备映射。
    pub(crate) device_vmas : Vec<DeviceVma>,
}

// The address space is accessed through MultiprocessorSafeCell.  The lock
// serializes the non-Send lazy-loader state as well as page-table mutation.
unsafe impl Send for Sv39AddressSpace {}
unsafe impl Sync for Sv39AddressSpace {}

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
                  loader : self.loader
                               .duplicate_box()? })
    }

    pub(crate) fn contains_page(&self, page : VirtAddr) -> bool {
        page.0 >= self.start.0 && page.0 < self.end.0
    }

    pub(crate) fn overlaps(&self, start : VirtAddr, end : VirtAddr) -> bool {
        start.0 < self.end.0 && end.0 > self.start.0
    }
}

// 本结构代码由AI完成
#[derive(Clone, Copy)]
pub(crate) struct SharedAnonVma {
    pub start : VirtAddr,
    pub end : VirtAddr,
}

pub(crate) struct SharedFileVma {
    pub start : VirtAddr,
    pub end : VirtAddr,
    pub file_offset : usize,
    pub loader : Box<dyn DemandPageLoader>,
}

#[derive(Clone)]
pub(crate) struct DeviceVma {
    pub start : VirtAddr,
    pub end : VirtAddr,
    pub phys_start : PhysPageNum,
    pub perm : PagePerm,
    pub lease : Arc<dyn DeviceMappingLease>,
}

impl DeviceVma {
    pub(crate) fn contains_page(&self, page : VirtAddr) -> bool {
        page.0 >= self.start.0 && page.0 < self.end.0
    }

    pub(crate) fn overlaps(&self, start : VirtAddr, end : VirtAddr) -> bool {
        start.0 < self.end.0 && end.0 > self.start.0
    }
}

impl SharedFileVma {
    fn duplicate(&self) -> MmResult<Self> {
        Ok(Self { start : self.start,
                  end : self.end,
                  file_offset : self.file_offset,
                  loader : self.loader.duplicate_box()? })
    }

    pub(crate) fn overlaps(&self, start : VirtAddr, end : VirtAddr) -> bool {
        start.0 < self.end.0 && end.0 > self.start.0
    }
}

impl SharedAnonVma {
    pub(crate) fn contains_page(&self, page : VirtAddr) -> bool {
        page.0 >= self.start.0 && page.0 < self.end.0
    }

    pub(crate) fn overlaps(&self, start : VirtAddr, end : VirtAddr) -> bool {
        start.0 < self.end.0 && end.0 > self.start.0
    }
}

impl Sv39AddressSpace {
    /// 分配并清零根页表帧；依赖帧分配器与 [`table_mut`] 的物理访问假设。
    pub(crate) fn new() -> MmResult<Self> {
        let asid = crate::asid::allocate_user()?;
        let root = match alloc_table_frame_zeroed() {
            Ok(root) => root,
            Err(error) => {
                crate::asid::release_user(asid);
                return Err(error);
            }
        };
        Ok(Self { root,
                  asid,
                  user_brk_start : VirtAddr(0),
                  user_brk_current_end : VirtAddr(0),
                  user_brk_max : VirtAddr(0),
                  mmap_anon_cursor : VirtAddr(0),
                  mmap_file_cursor : VirtAddr(0),
                  mmap_base : VirtAddr(0),
                  user_stack_bottom : VirtAddr(0),
                  user_stack_top : VirtAddr(0),
                  lazy_file_vmas : Vec::new(),
                  shared_anon_vmas : Vec::new(),
                  shared_file_vmas : Vec::new(),
                  device_vmas : Vec::new() })
    }

    /// 创建内核地址空间；ASID 0 不参与用户编号复用。
    pub(crate) fn new_kernel() -> MmResult<Self> {
        let root = alloc_table_frame_zeroed()?;
        Ok(Self { root,
                  asid : crate::asid::KERNEL_ASID,
                  user_brk_start : VirtAddr(0),
                  user_brk_current_end : VirtAddr(0),
                  user_brk_max : VirtAddr(0),
                  mmap_anon_cursor : VirtAddr(0),
                  mmap_file_cursor : VirtAddr(0),
                  mmap_base : VirtAddr(0),
                  user_stack_bottom : VirtAddr(0),
                  user_stack_top : VirtAddr(0),
                  lazy_file_vmas : Vec::new(),
                  shared_anon_vmas : Vec::new(),
                  shared_file_vmas : Vec::new(),
                  device_vmas : Vec::new() })
    }

    pub(crate) fn kernel_satp_value(&self) -> usize {
        make_satp(self.root, crate::asid::KERNEL_ASID as usize)
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
        self.user_stack_bottom
            .0 <
        self.user_stack_top
            .0 &&
        start.0 <
        self.user_stack_top
            .0 &&
        end.0 >
        self.user_stack_bottom
            .0
    }

    fn stack_overlap_end(&self, start : VirtAddr, end : VirtAddr) -> Option<VirtAddr> {
        self.range_overlaps_stack(start, end)
            .then(|| self.user_stack_top.ceil_page()
                                        .start_addr())
    }

    fn kernel_reserved_overlap_end(&self, start : VirtAddr, end : VirtAddr) -> Option<VirtAddr> {
        let reserved = [
            core::ptr::addr_of!(__alltraps) as usize,
            core::ptr::addr_of!(__wateros_riscv_restore_user_from_frame) as usize,
            core::ptr::addr_of!(__wateros_riscv_kernel_satp) as usize,
            core::ptr::addr_of!(__wateros_riscv_return_frame) as usize,
        ];
        reserved
            .iter()
            .copied()
            .filter(|addr| page_range_overlaps_addr(start, end, *addr))
            .map(|addr| VirtAddr(addr).floor_page()
                                      .start_addr()
                                      .0 + PAGE_SIZE)
            .max()
            .map(VirtAddr)
    }

    pub(crate) fn range_overlaps_kernel_reserved(&self, start : VirtAddr, end : VirtAddr) -> bool {
        page_range_overlaps_addr(start,
                                 end,
                                 core::ptr::addr_of!(__alltraps) as usize) ||
        page_range_overlaps_addr(start,
                                 end,
                                 core::ptr::addr_of!(__wateros_riscv_restore_user_from_frame)
                                 as usize) ||
        page_range_overlaps_addr(start,
                                 end,
                                 core::ptr::addr_of!(__wateros_riscv_kernel_satp) as usize) ||
        page_range_overlaps_addr(start,
                                 end,
                                 core::ptr::addr_of!(__wateros_riscv_return_frame) as usize)
    }

    pub(crate) fn validate_user_mapping_range(&self,
                                              start : VirtAddr,
                                              end : VirtAddr)
                                              -> MmResult<()> {
        if start.0 >= end.0 || end.0 > USER_VA_LIMIT {
            return Err(MmError::InvalidAddress);
        }
        if self.range_overlaps_stack(start, end) || self.range_overlaps_kernel_reserved(start, end)
        {
            return Err(MmError::InvalidAddress);
        }
        Ok(())
    }

    pub(crate) fn lazy_vma_overlaps(&self, start : VirtAddr, end : VirtAddr) -> bool {
        if start.0 >= end.0 {
            return false;
        }
        // `lazy_file_vmas` is sorted and non-overlapping, so every VMA before
        // this partition ends to the left of the query.  Only the first
        // remaining VMA can decide whether an overlap exists.
        let index = self.lazy_file_vmas
                        .partition_point(|vma| vma.end.0 <= start.0);
        self.lazy_file_vmas
            .get(index)
            .is_some_and(|vma| vma.start.0 < end.0)
    }

    fn lazy_vma_overlap_end(&self, start : VirtAddr, end : VirtAddr) -> Option<VirtAddr> {
        let mut low = 0usize;
        let mut high = self.lazy_file_vmas.len();
        while low < high {
            let mid = low + (high - low) / 2;
            if self.lazy_file_vmas[mid].end.0 <= start.0 {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        let vma = self.lazy_file_vmas.get(low)?;
        vma.overlaps(start, end)
            .then_some(vma.end)
    }

    pub(crate) fn lazy_vma_contains(&self, page : VirtAddr) -> bool {
        self.lazy_file_vmas
            .iter()
            .any(|vma| vma.contains_page(page))
    }

    pub(crate) fn merge_lazy_file_vma_perm(&mut self,
                                           start : VirtAddr,
                                           end : VirtAddr,
                                           perm : PagePerm)
                                           -> MmResult<()> {
        let mut next = Vec::new();
        for vma in self.lazy_file_vmas
                       .drain(..)
        {
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
        self.shared_anon_vmas
            .iter()
            .any(|vma| vma.overlaps(start, end))
    }

    fn shared_anon_vma_overlap_end(&self, start : VirtAddr, end : VirtAddr) -> Option<VirtAddr> {
        self.shared_anon_vmas
            .iter()
            .filter(|vma| vma.overlaps(start, end))
            .map(|vma| vma.end)
            .max_by_key(|vma_end| vma_end.0)
    }

    pub(crate) fn shared_vma_contains(&self, page : VirtAddr) -> bool {
        self.shared_anon_vmas
            .iter()
            .any(|vma| vma.contains_page(page))
    }

    /// 页是否由其他地址空间或设备持有，解除 PTE 时不得回收物理页。
    pub(crate) fn non_owned_vma_contains(&self, page : VirtAddr) -> bool {
        self.shared_vma_contains(page) ||
        self.device_vmas
            .iter()
            .any(|vma| vma.contains_page(page))
    }

    pub(crate) fn device_vma_overlaps(&self, start : VirtAddr, end : VirtAddr) -> bool {
        self.device_vmas.iter().any(|vma| vma.overlaps(start, end))
    }

    pub(crate) fn register_device_vma(&mut self, vma : DeviceVma) {
        let position = self.device_vmas.partition_point(|entry| entry.start.0 < vma.start.0);
        self.device_vmas.insert(position, vma);
    }

    pub(crate) fn remove_device_vmas(&mut self, start : VirtAddr, end : VirtAddr) {
        let mut next = Vec::new();
        for vma in self.device_vmas.drain(..) {
            if !vma.overlaps(start, end) {
                next.push(vma);
                continue;
            }
            if start.0 > vma.start.0 {
                next.push(DeviceVma { start : vma.start,
                                      end : start,
                                      phys_start : vma.phys_start,
                                      perm : vma.perm,
                                      lease : vma.lease.clone() });
            }
            if end.0 < vma.end.0 {
                let skipped_pages = (end.0 - vma.start.0) / PAGE_SIZE;
                next.push(DeviceVma { start : end,
                                      end : vma.end,
                                      phys_start : PhysPageNum(vma.phys_start.0 + skipped_pages),
                                      perm : vma.perm,
                                      lease : vma.lease });
            }
        }
        self.device_vmas = next;
    }

    pub(crate) fn protect_device_vmas(&mut self,
                                       start : VirtAddr,
                                       end : VirtAddr,
                                       perm : PagePerm) {
        let mut next = Vec::new();
        for vma in self.device_vmas.drain(..) {
            if !vma.overlaps(start, end) {
                next.push(vma);
                continue;
            }
            if start.0 > vma.start.0 {
                next.push(DeviceVma { start : vma.start,
                                      end : start,
                                      phys_start : vma.phys_start,
                                      perm : vma.perm,
                                      lease : vma.lease.clone() });
            }
            let mid_start = VirtAddr(core::cmp::max(start.0, vma.start.0));
            let mid_end = VirtAddr(core::cmp::min(end.0, vma.end.0));
            let mid_pages = (mid_start.0 - vma.start.0) / PAGE_SIZE;
            next.push(DeviceVma { start : mid_start,
                                  end : mid_end,
                                  phys_start : PhysPageNum(vma.phys_start.0 + mid_pages),
                                  perm,
                                  lease : vma.lease.clone() });
            if end.0 < vma.end.0 {
                let skipped_pages = (end.0 - vma.start.0) / PAGE_SIZE;
                next.push(DeviceVma { start : end,
                                      end : vma.end,
                                      phys_start : PhysPageNum(vma.phys_start.0 + skipped_pages),
                                      perm : vma.perm,
                                      lease : vma.lease });
            }
        }
        self.device_vmas = next;
    }

    /// 解除 mmap 区间；共享页和设备页只断开 PTE，其他页正常回收。
    pub(crate) fn unmap_mmap_range<A>(&mut self,
                                      allocator : &mut A,
                                      start : VirtAddr,
                                      end : VirtAddr)
                                      -> MmResult<()>
        where A : api_v0::frame_allocator::PhysicalFrameAllocator<FrameId = PhysPageNum>
    {
        let mut vpn = start.floor_page();
        let vpn_end = end.ceil_page();
        while vpn.0 < vpn_end.0 {
            if self.non_owned_vma_contains(vpn.start_addr()) {
                let _ = self.unmap_page_to_ppn(vpn)?;
            } else {
                self.unmap_page_with_alloc(allocator, vpn)?;
            }
            vpn = VirtPageNum(vpn.0 + 1);
        }
        Ok(())
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

    pub(crate) fn register_shared_file_vma(&mut self,
                                            start : VirtAddr,
                                            end : VirtAddr,
                                            file_offset : usize,
                                            loader : Box<dyn DemandPageLoader>) {
        self.shared_file_vmas.push(SharedFileVma { start,
                                                   end,
                                                   file_offset,
                                                   loader });
    }

    pub(crate) fn sync_shared_file_vmas(&mut self,
                                         start : VirtAddr,
                                         end : VirtAddr)
                                         -> MmResult<()> {
        let mut vmas = core::mem::take(&mut self.shared_file_vmas);
        let result = (|| {
            for vma in &mut vmas {
                if !vma.overlaps(start, end) {
                    continue;
                }
                let mut page = VirtAddr(core::cmp::max(start.0, vma.start.0)).floor_page()
                                                                                 .start_addr();
                let page_end = VirtAddr(core::cmp::min(end.0, vma.end.0)).ceil_page()
                                                                              .start_addr();
                while page.0 < page_end.0 {
                    if let Some(pa) = self.translate_addr(page)? {
                        let src = unsafe {
                            core::slice::from_raw_parts(pa.page_start().0 as *const u8, PAGE_SIZE)
                        };
                        let file_offset = vma.file_offset + (page.0 - vma.start.0);
                        vma.loader.write_page(file_offset, src)?;
                    }
                    page.0 += PAGE_SIZE;
                }
                vma.loader.flush()?;
            }
            Ok(())
        })();
        self.shared_file_vmas = vmas;
        result
    }

    pub(crate) fn remove_shared_file_vmas(&mut self,
                                           start : VirtAddr,
                                           end : VirtAddr)
                                           -> MmResult<()> {
        let mut next = Vec::new();
        for vma in self.shared_file_vmas.drain(..) {
            if !vma.overlaps(start, end) {
                next.push(vma);
                continue;
            }
            if start.0 > vma.start.0 {
                next.push(SharedFileVma { start : vma.start,
                                          end : start,
                                          file_offset : vma.file_offset,
                                          loader : vma.loader.duplicate_box()? });
            }
            if end.0 < vma.end.0 {
                next.push(SharedFileVma { start : end,
                                          end : vma.end,
                                          file_offset : vma.file_offset + (end.0 - vma.start.0),
                                          loader : vma.loader });
            }
        }
        self.shared_file_vmas = next;
        Ok(())
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
        let brk_guard = self.user_brk_current_end
                            .ceil_page()
                            .start_addr();
        let search_start = core::cmp::max(cursor.0,
                                          core::cmp::max(self.mmap_base.0, brk_guard.0));
        let mut base = VirtAddr(search_start).ceil_page()
                                             .start_addr();
        let mut skipped = 0usize;
        loop {
            if base.0 >= USER_VA_LIMIT {
                return Err(MmError::InvalidAddress);
            }
            if skipped > MAX_SEARCH_PAGES {
                return Err(MmError::InvalidAddress);
            }
            let end = VirtAddr(base.0
                                   .checked_add(n_pages.checked_mul(PAGE_SIZE)
                                                       .ok_or(MmError::InvalidAddress)?)
                                   .ok_or(MmError::InvalidAddress)?);
            if end.0 > USER_VA_LIMIT {
                let jump = VirtAddr(USER_VA_LIMIT);
                skipped = skipped.saturating_add((jump.0 - base.0).div_ceil(PAGE_SIZE));
                base = jump.ceil_page()
                           .start_addr();
                continue;
            }
            if let Some(jump) =
                self.stack_overlap_end(base, end)
                    .or_else(|| self.kernel_reserved_overlap_end(base, end))
            {
                let jump = VirtAddr(core::cmp::max(jump.0, base.0 + PAGE_SIZE));
                skipped = skipped.saturating_add((jump.0 - base.0).div_ceil(PAGE_SIZE));
                base = jump.ceil_page()
                           .start_addr();
                continue;
            }
            if let Some(jump) =
                self.lazy_vma_overlap_end(base, end)
                    .or_else(|| self.shared_anon_vma_overlap_end(base, end))
            {
                let jump = VirtAddr(core::cmp::max(jump.0, base.0 + PAGE_SIZE));
                skipped = skipped.saturating_add((jump.0 - base.0).div_ceil(PAGE_SIZE));
                base = jump.ceil_page()
                           .start_addr();
                continue;
            }
            let mut mapped_after = None;
            for i in 0..n_pages {
                let va = VirtAddr(base.0
                                      .checked_add(i.checked_mul(PAGE_SIZE)
                                                    .ok_or(MmError::InvalidAddress)?)
                                      .ok_or(MmError::InvalidAddress)?);
                if self.translate_addr(va)?.is_some() {
                    mapped_after = Some(va);
                    break;
                }
            }
            let Some(mapped) = mapped_after else {
                return Ok(base);
            };
            let jump = VirtAddr(core::cmp::min(mapped.0.saturating_add(PAGE_SIZE),
                                               USER_VA_LIMIT));
            skipped = skipped.saturating_add((jump.0 - base.0).div_ceil(PAGE_SIZE));
            base = jump.ceil_page()
                       .start_addr();
            if base.0 >= USER_VA_LIMIT {
                return Err(MmError::InvalidAddress);
            }
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
        let position = self.lazy_file_vmas
                           .partition_point(|vma| vma.start.0 < start.0);
        self.lazy_file_vmas
            .insert(position,
                    LazyFileVma { start,
                                  end,
                                  perm,
                                  file_offset,
                                  file_size,
                                  loader });
        Ok(())
    }

    pub(crate) fn remove_lazy_file_vmas(&mut self,
                                        start : VirtAddr,
                                        end : VirtAddr)
                                        -> MmResult<()> {
        let mut next = Vec::new();
        for vma in self.lazy_file_vmas
                       .drain(..)
        {
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
            if end.0 < vma.end.0 {
                let delta = end.0
                               .saturating_sub(vma.start.0);
                next.push(LazyFileVma { start : end,
                                        end : vma.end,
                                        perm : vma.perm,
                                        file_offset : vma.file_offset
                                                         .saturating_add(delta),
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
        if start.0 >= end.0 {
            return Ok(());
        }
        let first = self.lazy_file_vmas
                        .partition_point(|vma| vma.end.0 <= start.0);
        let last = self.lazy_file_vmas
                       .partition_point(|vma| vma.start.0 < end.0);
        if first >= last {
            return Ok(());
        }

        let first_vma = &self.lazy_file_vmas[first];
        let split_left = (start.0 > first_vma.start.0).then(|| {
            Ok::<_, MmError>(LazyFileVma { start : first_vma.start,
                                           end : start,
                                           perm : first_vma.perm,
                                           file_offset : first_vma.file_offset,
                                           file_size : first_vma.file_size,
                                           loader : first_vma.loader.duplicate_box()? })
        }).transpose()?;
        let last_vma = &self.lazy_file_vmas[last - 1];
        let split_right = (end.0 < last_vma.end.0).then(|| {
            Ok::<_, MmError>(LazyFileVma { start : end,
                                           end : last_vma.end,
                                           perm : last_vma.perm,
                                           file_offset : last_vma.file_offset +
                                                         (end.0 - last_vma.start.0),
                                           file_size : last_vma.file_size,
                                           loader : last_vma.loader.duplicate_box()? })
        }).transpose()?;

        if split_left.is_some() {
            let first_vma = &mut self.lazy_file_vmas[first];
            first_vma.file_offset += start.0 - first_vma.start.0;
            first_vma.start = start;
        }
        if split_right.is_some() {
            self.lazy_file_vmas[last - 1].end = end;
        }
        for vma in &mut self.lazy_file_vmas[first..last] {
            vma.perm = perm;
        }
        if let Some(right) = split_right {
            self.lazy_file_vmas.insert(last, right);
        }
        if let Some(left) = split_left {
            self.lazy_file_vmas.insert(first, left);
        }
        Ok(())
    }

    /// 沿 VPN 三级索引向下 walk，必要时分配中间页表；返回目标叶子 PTE 槽位。
    #[inline]
    fn walk_create(&mut self, vpn : VirtPageNum) -> MmResult<(&'static mut Sv39Pte, usize)> {
        let idx = vpn_indexes(vpn);
        let mut ppn = self.root;

        // 自根向叶：level 2 → 1 → 0；level=0 为待写入的 4 KiB 叶子槽
        for level in (0..SV39_LEVELS).rev() {
            let table = unsafe { table_mut(ppn) };
            let pte = &mut table[idx[level]];
            let flags = pte.flags();

            if level == 0 {
                return Ok((pte, level));
            }

            if !flags.is_valid() {
                // 中间节点空缺：分配清零子表，PTE 仅置 V（非 R/W/X 叶子）
                let child = alloc_table_frame_zeroed()?;
                pte.set(child, Sv39PteFlags::V);
            } else if flags.is_leaf() {
                // 中途命中叶子：VPN 与已有映射粒度冲突，无法继续向下
                return Err(MmError::AlreadyMapped);
            }

            // 进入下一级页表
            ppn = pte.ppn();
        }

        Err(MmError::InvalidAddress)
    }

    /// 只读 walk：找到叶子或中途停止；无效路径返回 `Ok(None)`。
    #[inline]
    fn walk_find(&self, vpn : VirtPageNum) -> MmResult<Option<(&'static mut Sv39Pte, usize)>> {
        let idx = vpn_indexes(vpn);
        let mut ppn = self.root;

        for level in (0..SV39_LEVELS).rev() {
            let table = unsafe { table_mut(ppn) };
            let pte = &mut table[idx[level]];
            let flags = pte.flags();

            if !flags.is_valid() {
                // 该级未映射，整段 VPN 视为未翻译
                return Ok(None);
            }
            if level == 0 || flags.is_leaf() {
                // 到达叶子层，或中途大页/叶 PTE
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
    // 本方法代码由AI完成
    pub fn fork_cow(&mut self) -> MmResult<Sv39AddressSpace> {
        log::trace!("[mm-fork] Sv39AddressSpace::fork begin root_ppn={}",
                    self.root.0);
        let child_lazy_file_vmas = self.lazy_file_vmas
                                       .iter()
                                       .map(LazyFileVma::duplicate)
                                       .collect::<MmResult<Vec<_>>>()?;
        let child_shared_file_vmas = self.shared_file_vmas
                                         .iter()
                                         .map(SharedFileVma::duplicate)
                                         .collect::<MmResult<Vec<_>>>()?;
        let child_asid = crate::asid::allocate_user()?;
        let child_root = match alloc_table_frame_zeroed() {
            Ok(root) => root,
            Err(error) => {
                crate::asid::release_user(child_asid);
                return Err(error);
            }
        };
        // SAFETY: 刚分配并清零的帧作为子地址空间根页表。
        if let Err(err) = unsafe {
            fork_table(self.root,
                       child_root,
                       SV39_LEVELS - 1,
                       0,
                       &self.shared_anon_vmas,
                       &self.device_vmas)
        } {
            unsafe {
                destroy_table(child_root,
                              SV39_LEVELS - 1,
                              0,
                              &self.shared_anon_vmas,
                              &self.device_vmas);
            }
            crate::asid::release_user(child_asid);
            return Err(err);
        }
        platform::arch::paging::flush_address_space_translations();
        log::trace!("[mm-fork] Sv39AddressSpace::fork done child_root={}",
                    child_root.0);
        Ok(Sv39AddressSpace { root : child_root,
                              asid : child_asid,
                              user_brk_start : self.user_brk_start,
                              user_brk_current_end : self.user_brk_current_end,
                              user_brk_max : self.user_brk_max,
                              mmap_anon_cursor : self.mmap_anon_cursor,
                              mmap_file_cursor : self.mmap_file_cursor,
                              mmap_base : self.mmap_base,
                              user_stack_bottom : self.user_stack_bottom,
                              user_stack_top : self.user_stack_top,
                              lazy_file_vmas : child_lazy_file_vmas,
                              shared_anon_vmas : self.shared_anon_vmas.clone(),
                              shared_file_vmas : child_shared_file_vmas,
                              device_vmas : self.device_vmas.clone() })
    }

    /// 递归释放所有用户页帧及页表帧，不触碰内核恒等映射。
    ///
    /// 调用后本地址空间不再可用。
    fn destroy_page_tables(&mut self) {
        if self.root.0 == 0 {
            return;
        }
        unsafe {
            destroy_table(self.root,
                          SV39_LEVELS - 1,
                          0,
                          &self.shared_anon_vmas,
                          &self.device_vmas);
        }
        self.root = PhysPageNum(0);
    }

    /// 释放页表并转移 ASID 所有权。调用方须在归还非零 ASID 前完成 TLB 失效。
    pub(crate) fn destroy_and_take_asid(&mut self) -> u16 {
        if let Err(error) = self.sync_shared_file_vmas(VirtAddr(0), VirtAddr(USER_VA_LIMIT)) {
            log::warn!("[mm] shared file writeback during address-space destroy failed: {error:?}");
        }
        self.destroy_page_tables();
        // UserAddressSpaceCell remains as a small tombstone so stale raw
        // handles can observe `dropped` without a use-after-free. The VMA
        // vectors are no longer consulted after page-table teardown, so free
        // their backing allocations and demand-page loaders here.
        drop(core::mem::take(&mut self.lazy_file_vmas));
        drop(core::mem::take(&mut self.shared_anon_vmas));
        drop(core::mem::take(&mut self.shared_file_vmas));
        drop(core::mem::take(&mut self.device_vmas));
        core::mem::replace(&mut self.asid, crate::asid::KERNEL_ASID)
    }

    /// 对单页执行写时复制：仅处理已标记 COW 且曾为可写的用户叶映射。
    // 本方法代码由AI完成
    fn handle_cow_page(&mut self, vpn : VirtPageNum) -> MmResult<bool> {
        let Some((pte, level)) = self.walk_find(vpn)? else {
            return Ok(false);
        };
        let flags = pte.flags();
        if !flags.is_leaf_at_level(level) || !flags.cow() || !flags.cow_was_writable() {
            return Ok(false);
        }
        let old_ppn = pte.ppn();
        let new_flags = flags.restore_cow_writable();
        // 独占帧：原地恢复 W，无需复制
        if frame_ref_count(old_ppn).map_err(MmError::from)? <= 1 {
            pte.set(old_ppn, new_flags);
            return Ok(true);
        }

        // 共享帧：分配新页、复制 4 KiB、递减旧帧引用
        let new_ppn = frame_alloc_result().map_err(MmError::from)?;
        let src = old_ppn.0 * PAGE_SIZE;
        let dst = new_ppn.0 * PAGE_SIZE;
        unsafe {
            core::ptr::copy_nonoverlapping(src as *const u8,
                                           dst as *mut u8,
                                           PAGE_SIZE);
        }
        frame_dealloc_result(old_ppn).map_err(MmError::from)?;
        pte.set(new_ppn, new_flags);
        Ok(true)
    }

    // 本方法代码由AI完成
    pub fn handle_cow_fault(&mut self, fault_addr : VirtAddr) -> MmResult<bool> {
        let changed = self.handle_cow_page(fault_addr.floor_page())?;
        if changed {
            platform::arch::paging::flush_address_space_translations();
        }
        Ok(changed)
    }

    /// Same as [`Self::handle_cow_fault`], but the caller owns TLB invalidation.
    pub(crate) fn handle_cow_fault_no_flush(&mut self,
                                            fault_addr : VirtAddr)
                                            -> MmResult<bool> {
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
        let Some(index) = self.lazy_file_vma_index(page)
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
            platform::arch::paging::flush_tlb_local(
                platform::arch::paging::TlbFlushRange::Page { addr : page.0 });
            return Ok(true);
        }
        let file_offset = {
            let vma = &self.lazy_file_vmas[index];
            vma.file_offset + (page.0 - vma.start.0)
        };
        if !perm.writable() {
            if let Some(ppn) = self.lazy_file_vmas[index].loader
                                                            .load_shared_page(file_offset)?
            {
                if let Err(error) = self.map_page_to_ppn(page.floor_page(), ppn, perm) {
                    let _ = frame_dealloc_result(ppn);
                    return Err(error);
                }
                platform::arch::paging::flush_tlb_local(
                    platform::arch::paging::TlbFlushRange::Page { addr : page.0 });
                return Ok(true);
            }
        }
        let ppn = allocator.alloc_frame()?;
        let pa = ppn.0 * PAGE_SIZE;
        let dst = unsafe { core::slice::from_raw_parts_mut(pa as *mut u8, PAGE_SIZE) };
        dst.fill(0);
        if let Err(e) = self.lazy_file_vmas[index].loader
                                                  .load_page(file_offset, dst)
        {
            let _ = allocator.dealloc_frame(ppn);
            return Err(e);
        }
        if let Err(e) = self.map_page_to_ppn(page.floor_page(), ppn, perm) {
            let _ = allocator.dealloc_frame(ppn);
            return Err(e);
        }
        platform::arch::paging::flush_tlb_local(
            platform::arch::paging::TlbFlushRange::Page { addr : page.0 });
        Ok(true)
    }

    /// 在按 `start` 升序且互不重叠的 lazy VMA 集合中查找包含 `page` 的条目。
    ///
    /// 只保证 `start` 有序；先定位第一个 `end > page` 的 VMA，再验证包含关系。
    fn lazy_file_vma_index(&self, page : VirtAddr) -> Option<usize> {
        let mut low = 0usize;
        let mut high = self.lazy_file_vmas.len();
        while low < high {
            let mid = low + (high - low) / 2;
            if self.lazy_file_vmas[mid].end.0 <= page.0 {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        let vma = self.lazy_file_vmas.get(low)?;
        vma.contains_page(page)
            .then_some(low)
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
        if !flags.is_leaf_at_level(level) ||
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
            core::ptr::copy_nonoverlapping(src as *const u8,
                                           dst as *mut u8,
                                           PAGE_SIZE);
        }
        frame_dealloc_result(old_ppn).map_err(MmError::from)?;
        pte.set(new_ppn, flags.clear_cow());
        platform::arch::paging::flush_address_space_translations();
        Ok(true)
    }
}

/// 递归销毁页表树：释放 U 标志的叶子页对应的物理帧，并释放本地址空间拥有的页表帧。
///
/// # Safety
/// 调用方确保 `ppn` 指向有效的 4 KiB 页表帧。
unsafe fn destroy_table(ppn : PhysPageNum,
                        level : usize,
                        vpn_prefix : usize,
                        shared_anon_vmas : &[SharedAnonVma],
                        device_vmas : &[DeviceVma]) {
    let table = unsafe { table_mut(ppn) };
    for i in 0..SV39_ENTRIES {
        let pte = table[i];
        let flags = pte.flags();
        if !flags.is_valid() {
            continue;
        }
        let child_ppn = pte.ppn();

        if flags.is_leaf_at_level(level) {
            // 用户叶：回收物理帧；共享匿名 VMA 内页由其它地址空间仍引用
            if flags.to_page_perm()
                    .user()
            {
                let page = VirtPageNum(vpn_prefix | (i << (level * VPN_INDEX_BITS))).start_addr();
                let is_shared_anon = shared_anon_vmas
                    .iter()
                    .any(|vma| vma.contains_page(page));
                let is_device = device_vmas.iter().any(|vma| vma.contains_page(page));
                if !is_shared_anon && !is_device {
                    let _ = frame_dealloc_result(child_ppn);
                }
            }
            // 内核恒等叶（无 U）：不释放，仅断开本地址空间 PTE
        } else if level > 0 {
            // 中间页表：递归销毁子树
            let child_prefix = vpn_prefix | (i << (level * VPN_INDEX_BITS));
            unsafe {
                destroy_table(child_ppn,
                              level - 1,
                              child_prefix,
                              shared_anon_vmas,
                              device_vmas);
            }
        }
    }
    // 释放当前层页表帧本身
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
                     level : usize,
                     vpn_prefix : usize,
                     shared_anon_vmas : &[SharedAnonVma],
                     device_vmas : &[DeviceVma])
                     -> MmResult<()> {
    let parent_table = unsafe { table_mut(parent_ppn) };
    let child_table = unsafe { table_mut(child_ppn) };

    for i in 0..SV39_ENTRIES {
        let pte = parent_table[i];
        let flags = pte.flags();
        if !flags.is_valid() {
            continue;
        }
        let ppn = pte.ppn();

        if flags.is_leaf_at_level(level) {
            let perm = flags.to_page_perm();
            if perm.user() {
                let page = VirtPageNum(vpn_prefix | (i << (level * VPN_INDEX_BITS))).start_addr();
                let is_shared_anon = shared_anon_vmas
                    .iter()
                    .any(|vma| vma.contains_page(page));
                let is_device = device_vmas.iter().any(|vma| vma.contains_page(page));
                if !is_shared_anon && !is_device {
                    frame_inc_ref(ppn).map_err(MmError::from)?;
                }
                // 可写私有页：父子共享物理帧，父 PTE 清 W 并打 COW 标记
                let child_flags = if is_shared_anon || is_device {
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
                // 内核叶：子表与父表共享同一 PPN
                child_table[i].set(ppn, flags);
            }
        } else if level > 0 {
            // 中间节点：为子地址空间复制一整棵子页表
            let child_prefix = vpn_prefix | (i << (level * VPN_INDEX_BITS));
            let child_sub = alloc_table_frame_zeroed()?;
            if let Err(err) =
                unsafe {
                    fork_table(ppn,
                               child_sub,
                               level - 1,
                               child_prefix,
                               shared_anon_vmas,
                               device_vmas)
                }
            {
                unsafe {
                    destroy_table(child_sub,
                                  level - 1,
                                  child_prefix,
                                  shared_anon_vmas,
                                  device_vmas);
                }
                return Err(err);
            }
            child_table[i].set(child_sub, Sv39PteFlags::V);
        }
    }
    Ok(())
}

impl Sv39AddressSpace {
    pub(crate) fn translate_addr_with_perm(&self,
                                           va : VirtAddr)
                                           -> MmResult<Option<(PhysAddr, PagePerm)>> {
        let vpn = va.floor_page();
        let off = va.page_offset();
        let Some((pte, level)) = self.walk_find(vpn)? else {
            return Ok(None);
        };
        if !pte.flags()
               .is_leaf_at_level(level)
        {
            return Ok(None);
        }
        if level != 0 {
            return Err(MmError::Unsupported);
        }
        Ok(Some((PhysAddr(pte.ppn().0 * PAGE_SIZE + off),
                 pte.flags()
                    .to_page_perm())))
    }
}

impl AddressSpaceOps for Sv39AddressSpace {
    fn satp_value(&self) -> usize { make_satp(self.root, self.asid as usize) }

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
        let Some((pte, level)) = self.walk_find(vpn)? else {
            return Ok(None);
        };
        if !pte.flags()
               .is_leaf_at_level(level)
        {
            return Ok(None);
        }
        let old = pte.ppn();
        pte.clear();
        Ok(Some(old))
    }

    fn protect_page(&mut self, vpn : VirtPageNum, perm : PagePerm) -> MmResult<()> {
        let Some((pte, level)) = self.walk_find(vpn)? else {
            return Err(MmError::NotMapped);
        };
        if !pte.flags()
               .is_leaf_at_level(level)
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
               .is_leaf_at_level(level)
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
        if !pte.flags()
               .is_leaf_at_level(level)
        {
            return Ok(None);
        }
        Ok(Some(pte.flags()
                   .to_page_perm()))
    }

    fn fork(&self) -> MmResult<Self> { Err(MmError::Unsupported) }
}

impl Drop for Sv39AddressSpace {
    fn drop(&mut self) {
        // 未装入 UserAddressSpaceCell 的对象尚未被调度，不会在远端 hart 留下
        // TLB 项，因此可以直接归还 ASID。
        let asid = self.destroy_and_take_asid();
        crate::asid::release_user(asid);
    }
}

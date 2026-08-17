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
//! 可写映射预先置 PTE **D** 位（脏位），只读映射必须保持 D=0，确保用户 store
//! 触发 Page Modified 异常并进入权限/COW 处理。

extern crate alloc;

use alloc::{boxed::Box, vec::Vec};

use api_v0::addr::{PhysAddr, PhysPageNum, VirtAddr, VirtPageNum, PAGE_SIZE};
use api_v0::address_space::AddressSpaceOps;
use api_v0::error::{MmError, MmResult};
use api_v0::mmap::{DemandPageLoader, PageFaultAccess};
use api_v0::perm::PagePerm;

use frame_alloctor::{frame_alloc_result, frame_dealloc_result, frame_inc_ref, frame_ref_count};
pub(crate) use impl_common::{
    handle_lazy_file_fault, DeviceVma, LazyFileVma, LazyVmaAccess, LazyVmaSet, SharedAnonVma,
    SharedFileVma, VmaBacking,
};

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
    const fn dirty(self) -> bool { (self.0 & Self::D.0) != 0 }

    #[inline]
    const fn user(self) -> bool { (self.0 & Self::PLV_MASK) == Self::PLV_USER }

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
    fn clear_cow(self) -> Self { Self(self.0 & !(Self::COW.0 | Self::COW_WAS_WRITABLE.0)) }

    #[inline]
    fn restore_cow_writable(self) -> Self {
        let mut f = self.clear_cow();
        f.0 |= Self::W.0 | Self::D.0;
        f
    }

    /// 从 [`PagePerm`] 构造 PTE 标志：V=1, MAT=CoherentCached，PLV 由
    /// `perm.user()` 决定。LoongArch 的写权限由硬件 D 位执行，W 位只供软件遍历。
    #[inline]
    fn from_perm(perm : PagePerm) -> Self {
        let mut f = Self::V; // always valid
        f.0 |= Self::P.0; // page is present
        f.0 |= Self::MAT_CACHED;
        if perm.user() {
            f.0 |= Self::PLV_USER; // PLV = 3
        }
        // else PLV stays 0 (kernel-only)
        if perm.writable() {
            f.0 |= Self::W.0 | Self::D.0;
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
                            (self.0 &
                             (LoongArch64PteFlags::NR.0 |
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

/// 将 VPN 拆分为三级索引：`[VPN[0], VPN[1], VPN[2]]`，
/// 与 Sv39 `vpn_indexes` 语义相同。
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
unsafe fn table_mut(ppn : PhysPageNum) -> &'static mut [LoongArch64Pte; LOONGARCH64_ENTRIES] {
    let pa = ppn.0 * PAGE_SIZE;
    unsafe { &mut *(pa as *mut [LoongArch64Pte; LOONGARCH64_ENTRIES]) }
}

struct UserLeafPage {
    addr : usize,
    perm : PagePerm,
}

unsafe fn collect_user_leaf_pages(ppn : PhysPageNum,
                                  level : usize,
                                  vpn_prefix : usize,
                                  out : &mut Vec<UserLeafPage>) {
    let table = unsafe { table_mut(ppn) };
    for i in 0..LOONGARCH64_ENTRIES {
        let pte = table[i];
        if pte.0 == 0 {
            continue;
        }
        let flags = pte.flags();
        let prefix = vpn_prefix | (i << (level * VPN_INDEX_BITS));
        if flags.is_leaf() {
            let addr = VirtPageNum(prefix).start_addr()
                                          .0;
            let mut perm = flags.to_page_perm();
            if flags.cow_was_writable() {
                perm |= PagePerm::W;
            }
            if addr < USER_VA_LIMIT && perm.user() {
                out.push(UserLeafPage { addr, perm });
            }
        } else if level > 0 {
            unsafe { collect_user_leaf_pages(pte.ppn(), level - 1, prefix, out) };
        }
    }
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
    /// 硬件 TLB 地址空间标识；0 仅供内核地址空间使用。
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
    pub(crate) lazy_file_vmas : LazyVmaSet,
    pub(crate) shared_anon_vmas : Vec<SharedAnonVma>,
    pub(crate) shared_file_vmas : Vec<SharedFileVma>,
    /// 不属于通用帧分配器的外部设备映射。
    pub(crate) device_vmas : Vec<DeviceVma>,
}

// The address space is accessed through MultiprocessorSafeCell.  The lock
// serializes the non-Send lazy-loader state as well as page-table mutation.
unsafe impl Send for LoongArch64AddressSpace {}
unsafe impl Sync for LoongArch64AddressSpace {}

impl LazyVmaAccess for LoongArch64AddressSpace {
    fn lazy_vma_set(&self) -> &LazyVmaSet { &self.lazy_file_vmas }

    fn lazy_vma_set_mut(&mut self) -> &mut LazyVmaSet { &mut self.lazy_file_vmas }
}

impl LoongArch64AddressSpace {
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
                  lazy_file_vmas : LazyVmaSet::new(),
                  shared_anon_vmas : Vec::new(),
                  shared_file_vmas : Vec::new(),
                  device_vmas : Vec::new() })
    }

    /// 创建内核地址空间；ASID 0 不参与用户 ASID 分配与复用。
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
                  lazy_file_vmas : LazyVmaSet::new(),
                  shared_anon_vmas : Vec::new(),
                  shared_file_vmas : Vec::new(),
                  device_vmas : Vec::new() })
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
            .then(|| {
                self.user_stack_top
                    .ceil_page()
                    .start_addr()
            })
    }

    pub(crate) fn validate_user_mapping_range(&self,
                                              start : VirtAddr,
                                              end : VirtAddr)
                                              -> MmResult<()> {
        if start.0 >= end.0 || end.0 > USER_VA_LIMIT {
            return Err(MmError::InvalidAddress);
        }
        if self.range_overlaps_stack(start, end) {
            return Err(MmError::InvalidAddress);
        }
        Ok(())
    }

    /// 判断固定地址区间是否已经属于任意用户映射。
    ///
    /// PTE 扫描覆盖 ELF/brk 等 eager 映射；VMA 检查覆盖尚未缺页的 lazy 映射。
    pub(crate) fn user_mapping_range_occupied(&self,
                                              start : VirtAddr,
                                              end : VirtAddr)
                                              -> MmResult<bool> {
        if start.0 <
           self.user_brk_current_end
               .0 &&
           end.0 >
           self.user_brk_start
               .0
        {
            return Ok(true);
        }
        if self.lazy_vma_overlaps(start, end) ||
           self.shared_anon_vma_overlaps(start, end) ||
           self.device_vma_overlaps(start, end)
        {
            return Ok(true);
        }
        let mut vpn = start.floor_page();
        let end_vpn = end.ceil_page();
        while vpn.0 < end_vpn.0 {
            if self.translate_addr(vpn.start_addr())?
                   .is_some()
            {
                return Ok(true);
            }
            vpn = VirtPageNum(vpn.0 + 1);
        }
        Ok(false)
    }

    pub(crate) fn lazy_vma_overlaps(&self, start : VirtAddr, end : VirtAddr) -> bool {
        self.lazy_file_vmas
            .overlaps(start, end)
    }

    fn lazy_vma_overlap_end(&self, start : VirtAddr, end : VirtAddr) -> Option<VirtAddr> {
        self.lazy_file_vmas
            .overlap_end(start, end)
    }

    #[allow(dead_code)]
    pub(crate) fn lazy_vma_contains(&self, page : VirtAddr) -> bool {
        self.lazy_file_vmas
            .iter()
            .any(|vma| vma.contains_page(page))
    }

    #[allow(dead_code)]
    pub(crate) fn merge_lazy_file_vma_perm(&mut self,
                                           start : VirtAddr,
                                           end : VirtAddr,
                                           perm : PagePerm)
                                           -> MmResult<()> {
        self.lazy_file_vmas
            .merge_perm(start, end, perm)
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
        self.device_vmas
            .iter()
            .any(|vma| vma.overlaps(start, end))
    }

    pub(crate) fn register_device_vma(&mut self, vma : DeviceVma) {
        let position = self.device_vmas
                           .partition_point(|entry| entry.start.0 < vma.start.0);
        self.device_vmas
            .insert(position, vma);
    }

    pub(crate) fn remove_device_vmas(&mut self, start : VirtAddr, end : VirtAddr) {
        let mut next = Vec::new();
        for vma in self.device_vmas
                       .drain(..)
        {
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
                                      phys_start : PhysPageNum(vma.phys_start.0 +
                                                               skipped_pages),
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
        for vma in self.device_vmas
                       .drain(..)
        {
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
                                      phys_start : PhysPageNum(vma.phys_start.0 +
                                                               skipped_pages),
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
                                      -> MmResult<bool>
        where A : api_v0::frame_allocator::PhysicalFrameAllocator<FrameId = PhysPageNum>
    {
        let mut changed = false;
        let mut vpn = start.floor_page();
        let vpn_end = end.ceil_page();
        while vpn.0 < vpn_end.0 {
            if self.non_owned_vma_contains(vpn.start_addr()) {
                changed |= self.unmap_page_to_ppn(vpn)?.is_some();
            } else {
                changed |= self.unmap_page_with_alloc(allocator, vpn)?;
            }
            vpn = VirtPageNum(vpn.0 + 1);
        }
        Ok(changed)
    }

    pub(crate) fn register_shared_anon_vma(&mut self, start : VirtAddr, end : VirtAddr) {
        self.shared_anon_vmas
            .push(SharedAnonVma { start, end });
    }

    pub(crate) fn remove_shared_anon_vmas(&mut self, start : VirtAddr, end : VirtAddr) {
        let mut next = Vec::new();
        for vma in self.shared_anon_vmas
                       .drain(..)
        {
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
        self.shared_file_vmas
            .push(SharedFileVma { start,
                                  end,
                                  file_offset,
                                  backing : VmaBacking::File { loader } });
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
                            core::slice::from_raw_parts(pa.page_start().0 as *const u8,
                                                        PAGE_SIZE)
                        };
                        let file_offset = vma.file_offset + (page.0 - vma.start.0);
                        vma.backing
                           .write_page(file_offset, src)?;
                    }
                    page.0 += PAGE_SIZE;
                }
                vma.backing
                   .flush()?;
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
        for vma in self.shared_file_vmas
                       .drain(..)
        {
            if !vma.overlaps(start, end) {
                next.push(vma);
                continue;
            }
            if start.0 > vma.start.0 {
                next.push(SharedFileVma { start : vma.start,
                                          end : start,
                                          file_offset : vma.file_offset,
                                          backing : vma.backing
                                                       .duplicate()? });
            }
            if end.0 < vma.end.0 {
                next.push(SharedFileVma { start : end,
                                          end : vma.end,
                                          file_offset : vma.file_offset + (end.0 - vma.start.0),
                                          backing : vma.backing });
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
            if let Some(jump) = self.stack_overlap_end(base, end) {
                let jump = VirtAddr(core::cmp::max(jump.0, base.0 + PAGE_SIZE));
                skipped = skipped.saturating_add((jump.0 - base.0).div_ceil(PAGE_SIZE));
                base = jump.ceil_page()
                           .start_addr();
                continue;
            }
            if let Some(jump) = self.lazy_vma_overlap_end(base, end)
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
                if self.translate_addr(va)?
                       .is_some()
                {
                    mapped_after = Some(va);
                    break;
                }
            }
            let Some(mapped) = mapped_after else {
                return Ok(base);
            };
            let jump = VirtAddr(core::cmp::min(mapped.0
                                                     .saturating_add(PAGE_SIZE),
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
                                         backing : VmaBacking)
                                         -> MmResult<()> {
        self.validate_user_mapping_range(start, end)?;
        if self.lazy_vma_overlaps(start, end) {
            return Err(MmError::InvalidAddress);
        }
        self.ensure_lazy_refill_paths(start, end)?;
        let position = self.lazy_file_vmas
                           .partition_point(|vma| vma.start.0 < start.0);
        self.lazy_file_vmas
            .insert(position, LazyFileVma { start,
                                            end,
                                            perm,
                                            file_offset,
                                            file_size,
                                            backing });
        self.lazy_file_vmas
            .sort();
        Ok(())
    }

    /// Allocate the directory levels needed by the hardware refill walker.
    ///
    /// Linux points every empty directory slot at shared invalid lower-level
    /// tables. WaterOS uses zero-filled directories instead, so lazy VMAs must
    /// materialize their directory path while keeping the leaf PTE invalid.
    fn ensure_lazy_refill_paths(&mut self, start : VirtAddr, end : VirtAddr) -> MmResult<()> {
        const LEAF_TABLE_SPAN : usize = PAGE_SIZE * LOONGARCH64_ENTRIES;

        let last = end.0
                      .checked_sub(1)
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

    pub(crate) fn remove_lazy_file_vmas(&mut self,
                                        start : VirtAddr,
                                        end : VirtAddr)
                                        -> MmResult<()> {
        self.lazy_file_vmas
            .remove_range(start, end)
    }

    pub(crate) fn protect_lazy_file_vmas(&mut self,
                                         start : VirtAddr,
                                         end : VirtAddr,
                                         perm : PagePerm)
                                         -> MmResult<()> {
        self.lazy_file_vmas
            .protect_range(start, end, perm)
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
        let child_lazy_file_vmas = LazyVmaSet::from_vec(self.lazy_file_vmas
                                                            .iter()
                                                            .map(LazyFileVma::duplicate)
                                                            .collect::<MmResult<Vec<_>>>()?);
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
                       LOONGARCH64_LEVELS - 1,
                       0,
                       &self.shared_anon_vmas,
                       &self.device_vmas)
        } {
            unsafe {
                destroy_table(child_root,
                              LOONGARCH64_LEVELS - 1,
                              0,
                              &self.shared_anon_vmas,
                              &self.device_vmas);
            }
            crate::asid::release_user(child_asid);
            return Err(err);
        }
        platform::arch::paging::flush_address_space_translations();
        log::trace!("[mm-fork] LoongArch64AddressSpace::fork done child_root={}",
                    child_root.0);
        Ok(LoongArch64AddressSpace { root : child_root,
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
                                     shared_anon_vmas : self.shared_anon_vmas
                                                            .clone(),
                                     shared_file_vmas : child_shared_file_vmas,
                                     device_vmas : self.device_vmas
                                                       .clone() })
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
            return Ok(true);
        }

        // 引用计数 > 1：复制整页并切换 PTE 指向新帧
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
    pub(crate) fn handle_cow_fault_no_flush(&mut self, fault_addr : VirtAddr) -> MmResult<bool> {
        let vpn = fault_addr.floor_page();
        if self.handle_cow_page(vpn)? {
            return Ok(true);
        }

        // Another CPU may have resolved the same COW page after this CPU took
        // a PME on a stale D=0 TLB entry but before it acquired the address-space
        // lock.  In that case the current PTE is already a writable, dirty user
        // leaf.  Report the fault as handled so the outer wrapper invalidates
        // this CPU's stale translation (and conservatively shoots down peers)
        // before retrying the store.  A genuinely read-only or unmapped page
        // still returns false and is delivered as SIGSEGV by the trap layer.
        let Some((pte, level)) = self.walk_find(vpn)? else {
            return Ok(false);
        };
        let flags = pte.flags();
        Ok(level == 0 && flags.is_leaf() && flags.writable() && flags.dirty() && flags.user())
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
        let handled = handle_lazy_file_fault(self, allocator, fault_addr, access)?;
        if handled {
            platform::arch::paging::flush_tlb_local(
                platform::arch::paging::TlbFlushRange::Page { addr : page.0 });
        }
        Ok(handled)
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
            core::ptr::copy_nonoverlapping(src as *const u8,
                                           dst as *mut u8,
                                           PAGE_SIZE);
        }
        frame_dealloc_result(old_ppn).map_err(MmError::from)?;
        pte.set(new_ppn, flags.clear_cow());
        platform::arch::paging::flush_address_space_translations();
        Ok(true)
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
                          LOONGARCH64_LEVELS - 1,
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
        // Keep only the UserAddressSpaceCell tombstone needed by stale raw
        // handles; mappings and their demand-page loaders are dead now.
        drop(self.lazy_file_vmas
                 .take());
        drop(core::mem::take(&mut self.shared_anon_vmas));
        drop(core::mem::take(&mut self.shared_file_vmas));
        drop(core::mem::take(&mut self.device_vmas));
        core::mem::replace(&mut self.asid, crate::asid::KERNEL_ASID)
    }
}

/// 递归销毁页表树：释放 PLV==3（用户）的叶子页对应的物理帧，并释放本地址空间拥有的页表帧。
///
/// # Safety
/// 调用方确保 `ppn` 指向有效的 4 KiB 页表帧。
unsafe fn destroy_table(ppn : PhysPageNum,
                        level : usize,
                        vpn_prefix : usize,
                        shared_anon_vmas : &[SharedAnonVma],
                        device_vmas : &[DeviceVma]) {
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
                let is_device = device_vmas.iter()
                                           .any(|vma| vma.contains_page(page));
                if !is_shared_anon && !is_device {
                    let _ = frame_dealloc_result(child_ppn);
                }
            }
        } else if level > 0 {
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
                     shared_anon_vmas : &[SharedAnonVma],
                     device_vmas : &[DeviceVma])
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
                let is_device = device_vmas.iter()
                                           .any(|vma| vma.contains_page(page));
                if !is_shared_anon && !is_device {
                    frame_inc_ref(ppn).map_err(MmError::from)?;
                }
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
                child_table[i].set(ppn, flags);
            }
        } else if level > 0 {
            let child_prefix = vpn_prefix | (i << (level * VPN_INDEX_BITS));
            let child_sub = alloc_table_frame_zeroed()?;
            if let Err(err) = unsafe {
                fork_table(ppn,
                           child_sub,
                           level - 1,
                           child_prefix,
                           shared_anon_vmas,
                           device_vmas)
            } {
                unsafe {
                    destroy_table(child_sub,
                                  level - 1,
                                  child_prefix,
                                  shared_anon_vmas,
                                  device_vmas);
                }
                return Err(err);
            }
            child_table[i].set_table(child_sub);
        }
    }
    Ok(())
}

impl LoongArch64AddressSpace {
    /// 汇总页表叶子与 VMA 元数据，供 procfs/debugger 只读观察。
    pub(crate) fn user_mapping_snapshot(&self) -> Vec<api_v0::user_mapping::UserMappingSnapshot> {
        use api_v0::mmap::DemandMappingKind;
        use api_v0::user_mapping::{UserMappingKind, UserMappingSnapshot};

        let mut leaves = Vec::new();
        unsafe {
            collect_user_leaf_pages(self.root,
                                    LOONGARCH64_LEVELS - 1,
                                    0,
                                    &mut leaves)
        };
        let resident_in = |start : usize, end : usize| {
            leaves.iter()
                  .filter(|leaf| leaf.addr >= start && leaf.addr < end)
                  .count()
        };
        let mut mappings = Vec::new();

        for vma in self.lazy_file_vmas
                       .iter()
        {
            let kind = match &vma.backing {
                VmaBacking::Anonymous => UserMappingKind::Anonymous,
                VmaBacking::File { loader } => match loader.mapping_kind() {
                    DemandMappingKind::Anonymous => UserMappingKind::Anonymous,
                    DemandMappingKind::File => UserMappingKind::File,
                },
            };
            mappings.push(UserMappingSnapshot { start : vma.start.0,
                                                end : vma.end.0,
                                                perm : vma.perm,
                                                shared : false,
                                                file_offset : vma.file_offset,
                                                resident_pages : resident_in(vma.start.0,
                                                                             vma.end.0),
                                                kind });
        }
        if self.user_brk_start
               .0 <
           self.user_brk_current_end
               .0
        {
            let start = self.user_brk_start
                            .floor_page()
                            .start_addr()
                            .0;
            let end = self.user_brk_current_end
                          .ceil_page()
                          .start_addr()
                          .0;
            mappings.push(UserMappingSnapshot { start,
                                                end,
                                                perm : PagePerm::R | PagePerm::W | PagePerm::U,
                                                shared : false,
                                                file_offset : 0,
                                                resident_pages : resident_in(start, end),
                                                kind : UserMappingKind::Heap });
        }
        if self.user_stack_bottom
               .0 <
           self.user_stack_top
               .0
        {
            let start = self.user_stack_bottom
                            .floor_page()
                            .start_addr()
                            .0;
            let end = self.user_stack_top
                          .ceil_page()
                          .start_addr()
                          .0;
            mappings.push(UserMappingSnapshot { start,
                                                end,
                                                perm : PagePerm::R | PagePerm::W | PagePerm::U,
                                                shared : false,
                                                file_offset : 0,
                                                resident_pages : resident_in(start, end),
                                                kind : UserMappingKind::Stack });
        }
        for vma in &self.device_vmas {
            mappings.push(UserMappingSnapshot { start : vma.start.0,
                                                end : vma.end.0,
                                                perm : vma.perm,
                                                shared : true,
                                                file_offset : 0,
                                                resident_pages : resident_in(vma.start.0,
                                                                             vma.end.0),
                                                kind : UserMappingKind::Device });
        }

        let mut leaf_mappings : Vec<UserMappingSnapshot> = Vec::new();
        for leaf in leaves {
            if mappings.iter()
                       .any(|mapping| leaf.addr >= mapping.start && leaf.addr < mapping.end)
            {
                continue;
            }
            let page = VirtAddr(leaf.addr);
            let shared_file = self.shared_file_vmas
                                  .iter()
                                  .find(|vma| vma.contains_page(page));
            let shared = shared_file.is_some() ||
                         self.shared_anon_vmas
                             .iter()
                             .any(|vma| vma.contains_page(page));
            let (kind, file_offset) = if let Some(vma) = shared_file {
                (UserMappingKind::File, vma.file_offset + (leaf.addr - vma.start.0))
            } else {
                (UserMappingKind::Anonymous, 0)
            };
            if let Some(previous) = leaf_mappings.last_mut() {
                let offset_contiguous = kind != UserMappingKind::File ||
                                        previous.file_offset + (previous.end - previous.start) ==
                                        file_offset;
                if previous.end == leaf.addr &&
                   previous.perm == leaf.perm &&
                   previous.shared == shared &&
                   previous.kind == kind &&
                   offset_contiguous
                {
                    previous.end += PAGE_SIZE;
                    previous.resident_pages += 1;
                    continue;
                }
            }
            leaf_mappings.push(UserMappingSnapshot { start : leaf.addr,
                                                     end : leaf.addr + PAGE_SIZE,
                                                     perm : leaf.perm,
                                                     shared,
                                                     file_offset,
                                                     resident_pages : 1,
                                                     kind });
        }
        mappings.extend(leaf_mappings);
        mappings.sort_by_key(|mapping| mapping.start);
        mappings
    }

    pub(crate) fn translate_addr_with_perm(&self,
                                           va : VirtAddr)
                                           -> MmResult<Option<(PhysAddr, PagePerm)>> {
        let vpn = va.floor_page();
        let off = va.page_offset();
        let Some((pte, level)) = self.walk_find(vpn)? else {
            return Ok(None);
        };
        if level != 0 ||
           !pte.flags()
               .is_leaf()
        {
            return Ok(None);
        }
        Ok(Some((PhysAddr(pte.ppn().0 *
                          PAGE_SIZE +
                          off),
                 pte.flags()
                    .to_page_perm())))
    }
}

impl AddressSpaceOps for LoongArch64AddressSpace {
    /// 返回 WaterOS LA 地址空间 token：低 48 位为 PGDL，高位携带 10 位 ASID。
    fn satp_value(&self) -> usize { crate::asid::encode_token(self.root.0 * PAGE_SIZE, self.asid) }

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
        // 未装入 UserAddressSpaceCell 的对象尚未被调度，不会在远端 CPU 留下
        // TLB 项，因此可以直接归还 ASID。
        let asid = self.destroy_and_take_asid();
        crate::asid::release_user(asid);
    }
}

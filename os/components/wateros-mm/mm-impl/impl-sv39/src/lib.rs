//! RISC-V **Sv39** 页表实现（三级页表、**仅 4 KiB 叶子页**）。
//!
//! ## 物理访问假设
//!
//! `table_mut` 将 PPN 转为指针读写页表，要求 **PPN 对应物理内存在内核视角下可直接访问**（常见 bring-up：内核恒等映射 RAM/MMIO）。若改为偏移映射，必须同步改写该路径。
//!
//! ## 与 trap / 访问位
//!
//! 映射时预先置 PTE **A/D** 位，避免依赖 S 态 load/store 触发页故障（page fault）来置位（早期“先跑通”策略；若后续启用 demand paging 或严格 A/D 语义，应调整 `Sv39PteFlags::from_perm`）。

#![no_std]

use api_v0::addr::{PhysAddr, PhysPageNum, VirtAddr, VirtPageNum, PAGE_SIZE};
use api_v0::address_space::AddressSpaceOps;
use api_v0::error::{MmError, MmResult};
use api_v0::perm::PagePerm;

use frame_alloctor::{frame_alloc_result, frame_dealloc_result};
use wateros_base::addr::BasePPN;

/// Sv39 PTE flags（硬件编码语义）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Sv39PteFlags(u16);

impl Sv39PteFlags {
    const V: Self = Self(1 << 0);
    const R: Self = Self(1 << 1);
    const W: Self = Self(1 << 2);
    const X: Self = Self(1 << 3);
    const U: Self = Self(1 << 4);
    // const G: Self = Self(1 << 5);
    const A: Self = Self(1 << 6);
    const D: Self = Self(1 << 7);

    #[inline]
    const fn empty() -> Self { Self(0) }

    #[inline]
    const fn bits(self) -> u16 { self.0 }

    #[inline]
    const fn is_valid(self) -> bool { (self.0 & Self::V.0) != 0 }

    #[inline]
    const fn is_leaf(self) -> bool { (self.0 & (Self::R.0 | Self::W.0 | Self::X.0)) != 0 }

    #[inline]
    fn from_perm(perm: PagePerm) -> Self {
        let mut f = Self::empty();
        f.0 |= Self::V.0;
        if perm.readable() { f.0 |= Self::R.0; }
        if perm.writable() { f.0 |= Self::W.0; }
        if perm.executable() { f.0 |= Self::X.0; }
        if perm.user() { f.0 |= Self::U.0; }
        // early: 直接置 A/D，避免后续访问触发异常（更贴合“先跑通”）
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

/// Sv39 页表项（64-bit）
#[repr(transparent)]
#[derive(Clone, Copy)]
struct Sv39Pte(usize);

impl Sv39Pte {
    #[inline]
    const fn zero() -> Self { Self(0) }

    #[inline]
    fn flags(self) -> Sv39PteFlags { Sv39PteFlags((self.0 & 0x3ff) as u16) }

    #[inline]
    fn ppn(self) -> PhysPageNum { PhysPageNum((self.0 >> 10) & ((1usize << 44) - 1)) }

    #[inline]
    fn set(&mut self, ppn: PhysPageNum, flags: Sv39PteFlags) {
        self.0 = (ppn.0 << 10) | (flags.bits() as usize);
    }

    #[inline]
    fn clear(&mut self) { self.0 = 0; }
}

const SV39_LEVELS: usize = 3;
const SV39_ENTRIES: usize = 512;

#[inline]
fn vpn_indexes(vpn: VirtPageNum) -> [usize; 3] {
    let v = vpn.0;
    [
        (v >> 0) & 0x1ff,
        (v >> 9) & 0x1ff,
        (v >> 18) & 0x1ff,
    ]
}

/// 将页表帧 PPN 映射为可变的 Sv39 PTE 数组。
///
/// # Safety
///
/// 调用方保证 `ppn` 指向已映射且 **4 KiB 对齐** 的页表存储；本函数用 `ppn * PAGE_SIZE` 作为 **恒等或线性物理地址** 解引用。
#[inline]
unsafe fn table_mut(ppn: PhysPageNum) -> &'static mut [Sv39Pte; SV39_ENTRIES] {
    // early stage：假设物理内存可直接线性访问（裸机/恒等映射环境）。
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

/// Sv39 根页表与 walk 状态；所有映射均为 **4 KiB 叶子**（`translate_addr` 在 level≠0 叶子时返回 `MmError::Unsupported`）。
pub struct Sv39AddressSpace {
    root: PhysPageNum,
}

impl Sv39AddressSpace {
    /// 分配并清零根页表帧；依赖帧分配器与 `table_mut` 的物理访问假设。
    pub fn new() -> MmResult<Self> {
        let root = alloc_table_frame_zeroed()?;
        Ok(Self { root })
    }

    #[inline]
    fn walk_create(&mut self, vpn: VirtPageNum) -> MmResult<(&'static mut Sv39Pte, usize)> {
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
                // 遇到叶子就无法继续下钻
                return Err(MmError::AlreadyMapped);
            }

            ppn = pte.ppn();
        }

        Err(MmError::InvalidAddress)
    }

    #[inline]
    fn walk_find(&self, vpn: VirtPageNum) -> MmResult<Option<(&'static mut Sv39Pte, usize)>> {
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

    /// 已映射叶子页的 [`PagePerm`]（仅 level-0 叶子）；用于 ELF 相邻 `PT_LOAD` 同页合并权限。
    pub(crate) fn leaf_perm(&self, vpn: VirtPageNum) -> MmResult<Option<PagePerm>> {
        let Some((pte, level)) = self.walk_find(vpn)? else {
            return Ok(None);
        };
        if level != 0 || !pte.flags().is_leaf() {
            return Ok(None);
        }
        Ok(Some(pte.flags().to_page_perm()))
    }
}

impl AddressSpaceOps for Sv39AddressSpace {
    fn satp_value(&self) -> usize {
        // satp: MODE=8 (Sv39), ASID=0, PPN=root（根表须为 4K 对齐物理帧）
        (8usize << 60) | (self.root.0 & ((1usize << 44) - 1))
    }

    fn map_page_to_ppn(
        &mut self,
        vpn: VirtPageNum,
        ppn: PhysPageNum,
        perm: PagePerm,
    ) -> MmResult<()> {
        let (pte, _level) = self.walk_create(vpn)?;
        if pte.flags().is_valid() {
            return Err(MmError::AlreadyMapped);
        }
        pte.set(ppn, Sv39PteFlags::from_perm(perm));
        Ok(())
    }

    fn unmap_page_to_ppn(&mut self, vpn: VirtPageNum) -> MmResult<Option<PhysPageNum>> {
        let Some((pte, _level)) = self.walk_find(vpn)? else {
            return Ok(None);
        };
        if !pte.flags().is_leaf() {
            // 不是叶子则不认为是有效页映射
            return Ok(None);
        }
        let old = pte.ppn();
        pte.clear();
        Ok(Some(old))
    }

    fn protect_page(&mut self, vpn: VirtPageNum, perm: PagePerm) -> MmResult<()> {
        let Some((pte, _level)) = self.walk_find(vpn)? else {
            return Err(MmError::NotMapped);
        };
        if !pte.flags().is_leaf() {
            return Err(MmError::NotMapped);
        }
        let ppn = pte.ppn();
        pte.set(ppn, Sv39PteFlags::from_perm(perm));
        Ok(())
    }

    fn translate_addr(&self, va: VirtAddr) -> MmResult<Option<PhysAddr>> {
        let vpn = va.floor_page();
        let off = va.page_offset();
        let Some((pte, level)) = self.walk_find(vpn)? else {
            return Ok(None);
        };
        if !pte.flags().is_leaf() {
            return Ok(None);
        }

        // 当前实现只做 4KiB 页（level==0 叶子）；大页可后续再加
        if level != 0 {
            return Err(MmError::Unsupported);
        }
        Ok(Some(PhysAddr(pte.ppn().0 * PAGE_SIZE + off)))
    }
}

// 早期阶段：允许手动回收根表（不做递归回收中间表）
impl Drop for Sv39AddressSpace {
    fn drop(&mut self) {
        let _ = frame_dealloc_result(self.root);
    }
}

/// Sv39 与帧分配器自测；`start_ppn`/`end_ppn` 与 [`frame_alloctor::init_frame_allocator`] 语义一致（PPN 半开区间由平台给出）。
pub fn test_with_range(start_ppn: BasePPN, end_ppn: BasePPN) {
    log::trace!("[mm-impl::sv39] test begin");
    frame_alloctor::test_with_range(start_ppn, end_ppn);

    let mut aspace = Sv39AddressSpace::new().expect("Sv39AddressSpace::new should succeed");
    let satp = aspace.satp_value();
    assert_eq!(satp >> 60, 8, "satp mode should be Sv39");

    // 取一页测试映射
    let ppn = frame_alloc_result().expect("alloc one frame for map test");
    let vpn = VirtPageNum(0x200);
    let perm = PagePerm::R | PagePerm::W | PagePerm::U;
    aspace.map_page_to_ppn(vpn, ppn, perm).expect("map should succeed");
    let map_dup = aspace.map_page_to_ppn(vpn, ppn, perm);
    assert!(matches!(map_dup, Err(MmError::AlreadyMapped)));

    let va = VirtAddr(vpn.0 * PAGE_SIZE + 0x123);
    let pa = aspace.translate_addr(va).expect("translate should not error").expect("should map");
    assert_eq!(pa.0, ppn.0 * PAGE_SIZE + 0x123);

    // 修改权限不应影响翻译结果
    aspace
        .protect_page(vpn, PagePerm::R | PagePerm::U)
        .expect("protect should succeed");
    let pa2 = aspace.translate_addr(va).unwrap().unwrap();
    assert_eq!(pa2.0, pa.0);

    // 解除映射应返回 ppn
    let old = aspace.unmap_page_to_ppn(vpn).expect("unmap should succeed");
    assert_eq!(old, Some(ppn));
    let none = aspace.translate_addr(va).unwrap();
    assert!(none.is_none());
    let missing_protect = aspace.protect_page(vpn, PagePerm::R);
    assert!(matches!(missing_protect, Err(MmError::NotMapped)));
    let second_unmap = aspace.unmap_page_to_ppn(vpn).expect("second unmap should be ok");
    assert!(second_unmap.is_none());

    // 回收测试页
    frame_dealloc_result(ppn).expect("dealloc test frame");

    log::trace!("[mm-impl::sv39] test end");
}

mod kernel_elf;
mod kernel_global;

/// 内核全局页表与用户 ELF 装载（QEMU RISC-V bring-up）；由 `wateros-mm` 聚合为 `mm::kernel_mm`。
pub mod kernel_mm_impl {
    pub use crate::kernel_elf::{from_elf_bytes, from_elf_path};
    pub use crate::kernel_global::{
        ensure_user_execute_for_kernel_va, init, kernel_satp, map_anon_range_user,
        map_identity_range_user,
    };
}

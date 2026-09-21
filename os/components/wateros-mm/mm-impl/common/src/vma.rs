//! 双架构共享的用户 VMA 模型与有序注册表。
//!
//! 页表只描述已经驻留的硬件映射；[`VmaSet`] 是地址空间虚拟区间语义的唯一来源。
//! 每个 [`VmArea`] 同时携带范围、权限、共享方式、用途、缺页策略和后备对象，避免
//! lazy/file/shared/external/device 分别维护多份相互重叠的区间表。

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::ops::Bound::{Excluded, Unbounded};

use api_v0::addr::{PhysPageNum, VirtAddr, PAGE_SIZE};
use api_v0::error::{MmError, MmResult};
use api_v0::mmap::{DemandPageLoader, DeviceMappingLease};
use api_v0::perm::PagePerm;

/// VMA 的用户可见用途。它只参与策略和诊断，不编码页表实现细节。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmaKind {
    Anonymous,
    File,
    Heap,
    Stack,
    /// SysV SHM 等由地址空间外部对象持有物理页的共享映射。
    SharedMemory,
    Device,
}

/// fork 与写入时采用私有（COW）还是共享物理页语义。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmaSharing {
    Private,
    Shared,
}

/// VMA 缺页时如何取得内容或保持外部对象生命周期。
pub enum VmaBacking {
    /// 匿名零页；目标帧由 fault 路径预先清零。
    Anonymous,
    /// 文件后备。eager 映射可以没有 loader；这种 VMA 不允许按需补页或写回。
    File {
        loader : Option<Box<dyn DemandPageLoader>>,
    },
    /// 不属于通用帧分配器引用计数的离散外部页。
    ///
    /// 物理页身份保存在外部对象（当前为 SysV SHM registry）中；VMA 只记录
    /// 所有权类别，防止 fork、munmap 和地址空间销毁错误增减普通 frame 引用。
    External,
    /// 不属于通用帧分配器的连续设备页。
    Device {
        phys_start : PhysPageNum,
        lease : Arc<dyn DeviceMappingLease>,
    },
}

impl VmaBacking {
    fn duplicate(&self) -> MmResult<Self> {
        Ok(match self {
            Self::Anonymous => Self::Anonymous,
            Self::File { loader } => Self::File { loader:
                                                      loader.as_ref()
                                                            .map(|loader| loader.duplicate_box())
                                                            .transpose()? },
            Self::External => Self::External,
            Self::Device { phys_start, lease } => Self::Device { phys_start : *phys_start,
                                                                 lease : lease.clone() },
        })
    }

    pub fn load_page(&mut self, file_offset : usize, dst : &mut [u8]) -> MmResult<()> {
        match self {
            Self::Anonymous => Ok(()),
            Self::File { loader: Some(loader), } => loader.load_page(file_offset, dst),
            Self::File { loader: None } | Self::External | Self::Device { .. } => {
                Err(MmError::Unsupported)
            }
        }
    }

    pub fn load_shared_page(&mut self, file_offset : usize) -> MmResult<Option<PhysPageNum>> {
        match self {
            Self::Anonymous => Ok(None),
            Self::File { loader: Some(loader), } => loader.load_shared_page(file_offset),
            Self::File { loader: None } | Self::External | Self::Device { .. } => {
                Err(MmError::Unsupported)
            }
        }
    }

    pub fn write_page(&mut self, file_offset : usize, src : &[u8]) -> MmResult<()> {
        match self {
            Self::File { loader: Some(loader), } => loader.write_page(file_offset, src),
            Self::Anonymous |
            Self::File { loader: None } |
            Self::External |
            Self::Device { .. } => Err(MmError::Unsupported),
        }
    }

    pub fn flush(&mut self) -> MmResult<()> {
        match self {
            Self::Anonymous => Ok(()),
            Self::File { loader: Some(loader), } => loader.flush(),
            Self::File { loader: None } | Self::External | Self::Device { .. } => {
                Err(MmError::Unsupported)
            }
        }
    }

    fn shift_start(&mut self, skipped_bytes : usize) -> MmResult<()> {
        if let Self::Device { phys_start, .. } = self {
            if skipped_bytes % PAGE_SIZE != 0 {
                return Err(MmError::InvalidAddress);
            }
            phys_start.0 = phys_start.0
                                     .checked_add(skipped_bytes / PAGE_SIZE)
                                     .ok_or(MmError::InvalidAddress)?;
        }
        Ok(())
    }
}

/// 一段连续且语义一致的用户虚拟地址区间，等价于 Linux 的一个 `vm_area_struct`。
pub struct VmArea {
    /// 页对齐的半开区间起点。
    pub start : VirtAddr,
    /// 页对齐的半开区间终点。
    pub end : VirtAddr,
    /// 该区间允许的用户页权限。
    pub perm : PagePerm,
    /// 私有/COW 或共享语义。
    pub sharing : VmaSharing,
    /// 映射用途。
    pub kind : VmaKind,
    /// 缺页时是否允许根据 backing 安装新页。
    pub demand_paged : bool,
    /// `start` 对应的文件偏移；非文件 VMA 恒为 0。
    pub file_offset : usize,
    /// 建立映射时的文件长度快照；非文件 VMA 恒为 0。
    pub file_size : usize,
    /// 内容或外部页的所有权来源。
    pub backing : VmaBacking,
}

impl VmArea {
    pub fn anonymous(start : VirtAddr,
                     end : VirtAddr,
                     perm : PagePerm,
                     sharing : VmaSharing,
                     kind : VmaKind,
                     demand_paged : bool)
                     -> Self {
        Self { start,
               end,
               perm,
               sharing,
               kind,
               demand_paged,
               file_offset : 0,
               file_size : 0,
               backing : VmaBacking::Anonymous }
    }

    pub fn file(start : VirtAddr,
                end : VirtAddr,
                perm : PagePerm,
                sharing : VmaSharing,
                demand_paged : bool,
                file_offset : usize,
                file_size : usize,
                loader : Option<Box<dyn DemandPageLoader>>)
                -> Self {
        Self { start,
               end,
               perm,
               sharing,
               kind : VmaKind::File,
               demand_paged,
               file_offset,
               file_size,
               backing : VmaBacking::File { loader } }
    }

    pub fn device(start : VirtAddr,
                  end : VirtAddr,
                  perm : PagePerm,
                  phys_start : PhysPageNum,
                  lease : Arc<dyn DeviceMappingLease>)
                  -> Self {
        Self { start,
               end,
               perm,
               sharing : VmaSharing::Shared,
               kind : VmaKind::Device,
               demand_paged : false,
               file_offset : 0,
               file_size : 0,
               backing : VmaBacking::Device { phys_start, lease } }
    }

    /// 建立由外部子系统持有物理页生命周期的共享 VMA。
    pub fn external(start : VirtAddr, end : VirtAddr, perm : PagePerm) -> Self {
        Self { start,
               end,
               perm,
               sharing : VmaSharing::Shared,
               kind : VmaKind::SharedMemory,
               demand_paged : false,
               file_offset : 0,
               file_size : 0,
               backing : VmaBacking::External }
    }

    pub fn duplicate(&self) -> MmResult<Self> {
        Ok(Self { start : self.start,
                  end : self.end,
                  perm : self.perm,
                  sharing : self.sharing,
                  kind : self.kind,
                  demand_paged : self.demand_paged,
                  file_offset : self.file_offset,
                  file_size : self.file_size,
                  backing : self.backing
                                .duplicate()? })
    }

    pub fn contains_page(&self, page : VirtAddr) -> bool {
        page.0 >= self.start.0 && page.0 < self.end.0
    }

    pub fn overlaps(&self, start : VirtAddr, end : VirtAddr) -> bool {
        start.0 < self.end.0 && end.0 > self.start.0
    }

    pub fn is_shared(&self) -> bool { self.sharing == VmaSharing::Shared }

    pub fn is_shared_file(&self) -> bool {
        self.kind == VmaKind::File && self.sharing == VmaSharing::Shared
    }

    pub fn is_device(&self) -> bool { self.kind == VmaKind::Device }

    pub fn is_external(&self) -> bool { matches!(self.backing, VmaBacking::External) }

    pub fn is_non_owned(&self) -> bool {
        matches!(self.backing,
                 VmaBacking::External | VmaBacking::Device { .. })
    }

    pub fn device_phys_start(&self) -> Option<PhysPageNum> {
        match &self.backing {
            VmaBacking::Device { phys_start, .. } => Some(*phys_start),
            VmaBacking::Anonymous | VmaBacking::File { .. } | VmaBacking::External => None,
        }
    }

    fn validate_new_start(&self, start : VirtAddr) -> MmResult<()> {
        if start.0 < self.start.0 || start.0 >= self.end.0 || start.0 % PAGE_SIZE != 0 {
            return Err(MmError::InvalidAddress);
        }
        let skipped = start.0 - self.start.0;
        if self.kind == VmaKind::File {
            self.file_offset
                .checked_add(skipped)
                .ok_or(MmError::InvalidAddress)?;
        }
        if let VmaBacking::Device { phys_start, .. } = &self.backing {
            phys_start.0
                      .checked_add(skipped / PAGE_SIZE)
                      .ok_or(MmError::InvalidAddress)?;
        }
        Ok(())
    }

    fn duplicate_range(&self, start : VirtAddr, end : VirtAddr) -> MmResult<Self> {
        if start.0 < self.start.0 || end.0 > self.end.0 || start.0 >= end.0 {
            return Err(MmError::InvalidAddress);
        }
        let mut copy = self.duplicate()?;
        copy.set_start(start)?;
        copy.end = end;
        Ok(copy)
    }

    fn set_start(&mut self, start : VirtAddr) -> MmResult<()> {
        self.validate_new_start(start)?;
        let skipped = start.0 - self.start.0;
        if self.kind == VmaKind::File {
            self.file_offset = self.file_offset
                                   .checked_add(skipped)
                                   .ok_or(MmError::InvalidAddress)?;
        }
        self.backing
            .shift_start(skipped)?;
        self.start = start;
        Ok(())
    }

    fn mergeable_with(&self, next : &Self) -> bool {
        if self.end != next.start ||
           self.perm != next.perm ||
           self.sharing != next.sharing ||
           self.kind != next.kind ||
           self.demand_paged != next.demand_paged
        {
            return false;
        }
        match (&self.backing, &next.backing) {
            (VmaBacking::Anonymous, VmaBacking::Anonymous) => true,
            // File backing 尚未暴露可比较的稳定对象 identity。即使偏移连续，也不能把
            // 来自两个文件或 ELF 段的 VMA 合并。
            _ => false,
        }
    }
}

/// 地址空间唯一的、有序且全局无重叠的 VMA 注册表。
pub struct VmaSet {
    /// 以 VMA 起点为 key 的范围索引。VMA 全局无重叠，因此包含某地址的 VMA
    /// 必然是 `..=addr` 的最后一个条目，无需额外维护最大终点。
    inner : BTreeMap<usize, VmArea>,
}

impl VmaSet {
    pub const fn new() -> Self { Self { inner : BTreeMap::new() } }

    pub fn iter(&self) -> alloc::collections::btree_map::Values<'_, usize, VmArea> {
        self.inner.values()
    }

    pub fn iter_mut(&mut self) -> alloc::collections::btree_map::ValuesMut<'_, usize, VmArea> {
        self.inner
            .values_mut()
    }

    /// 取得 [`Self::lookup`] 返回的起点 key 对应的 VMA。
    pub fn get(&self, index : usize) -> Option<&VmArea> {
        self.inner
            .get(&index)
    }

    pub fn get_mut(&mut self, index : usize) -> Option<&mut VmArea> {
        self.inner
            .get_mut(&index)
    }

    pub fn len(&self) -> usize { self.inner.len() }

    pub fn is_empty(&self) -> bool {
        self.inner
            .is_empty()
    }

    pub fn clear(&mut self) { self.inner.clear(); }

    pub fn take(&mut self) -> BTreeMap<usize, VmArea> { core::mem::take(&mut self.inner) }

    pub fn duplicate(&self) -> MmResult<Self> {
        let mut inner = BTreeMap::new();
        for vma in self.inner.values() {
            let copy = vma.duplicate()?;
            inner.insert(copy.start.0, copy);
        }
        Ok(Self { inner })
    }

    /// 插入一个新 VMA。任何重叠、零长或非页对齐范围都作为运行时错误拒绝。
    pub fn insert(&mut self, vma : VmArea) -> MmResult<()> {
        if vma.start.0 >= vma.end.0 || vma.start.0 % PAGE_SIZE != 0 || vma.end.0 % PAGE_SIZE != 0 {
            return Err(MmError::InvalidAddress);
        }
        if self.inner
               .range(..=vma.start.0)
               .next_back()
               .is_some_and(|(_, previous)| previous.end.0 > vma.start.0)
        {
            return Err(MmError::AlreadyMapped);
        }
        if self.inner
               .range(vma.start.0..)
               .next()
               .is_some_and(|(&next_start, _)| next_start < vma.end.0)
        {
            return Err(MmError::AlreadyMapped);
        }
        let start = vma.start.0;
        self.inner
            .insert(start, vma);
        self.coalesce_at(start);
        Ok(())
    }

    /// 查找包含给定页地址的 VMA，返回其稳定起点 key。
    pub fn lookup(&self, page : VirtAddr) -> Option<usize> {
        self.inner
            .range(..=page.0)
            .next_back()
            .filter(|(_, vma)| vma.contains_page(page))
            .map(|(&start, _)| start)
    }

    /// 返回第一个与 `[start, end)` 重叠的 VMA 起点 key。
    fn first_overlap_key(&self, start : VirtAddr, end : VirtAddr) -> Option<usize> {
        if start.0 >= end.0 {
            return None;
        }
        if let Some((&key, _)) = self.inner
                                     .range(..=start.0)
                                     .next_back()
                                     .filter(|(_, vma)| vma.end.0 > start.0)
        {
            return Some(key);
        }
        self.inner
            .range(start.0..end.0)
            .next()
            .map(|(&key, _)| key)
    }

    fn overlapping_keys(&self, start : VirtAddr, end : VirtAddr) -> Vec<usize> {
        let Some(first) = self.first_overlap_key(start, end) else {
            return Vec::new();
        };
        self.inner
            .range(first..end.0)
            .take_while(|(_, vma)| vma.start.0 < end.0)
            .map(|(&key, _)| key)
            .collect()
    }

    fn next_key(&self, key : usize) -> Option<usize> {
        self.inner
            .range((Excluded(key), Unbounded))
            .next()
            .map(|(&next, _)| next)
    }

    /// 只合并 `key` 附近的兼容 VMA，避免每次插入都重建整张范围索引。
    fn coalesce_at(&mut self, key : usize) -> usize {
        if !self.inner
                .contains_key(&key)
        {
            return key;
        }
        let mut current = key;
        let previous = self.inner
                           .range(..current)
                           .next_back()
                           .map(|(&previous, _)| previous);
        if let Some(previous) = previous {
            let merge = {
                let left = self.inner
                               .get(&previous)
                               .expect("VMA predecessor disappeared");
                let right = self.inner
                                .get(&current)
                                .expect("VMA entry disappeared");
                left.mergeable_with(right)
            };
            if merge {
                let right = self.inner
                                .remove(&current)
                                .expect("mergeable VMA disappeared");
                self.inner
                    .get_mut(&previous)
                    .expect("VMA predecessor disappeared")
                    .end = right.end;
                current = previous;
            }
        }
        loop {
            let Some(next) = self.next_key(current) else {
                break;
            };
            let merge = {
                let left = self.inner
                               .get(&current)
                               .expect("VMA entry disappeared");
                let right = self.inner
                                .get(&next)
                                .expect("VMA successor disappeared");
                left.mergeable_with(right)
            };
            if !merge {
                break;
            }
            let right = self.inner
                            .remove(&next)
                            .expect("mergeable VMA disappeared");
            self.inner
                .get_mut(&current)
                .expect("VMA entry disappeared")
                .end = right.end;
        }
        current
    }

    fn debug_assert_invariants(&self) {
        debug_assert!(self.inner
                          .iter()
                          .all(|(&key, vma)| key == vma.start.0));
        debug_assert!(self.inner
                          .values()
                          .zip(self.inner
                                   .values()
                                   .skip(1))
                          .all(|(left, right)| left.end.0 <= right.start.0));
    }

    fn insert_prepared(&mut self, vma : VmArea) {
        let start = vma.start.0;
        let replaced = self.inner
                           .insert(start, vma);
        debug_assert!(replaced.is_none());
    }

    fn remove_prepared(&mut self, keys : &[usize]) {
        for key in keys {
            let removed = self.inner
                              .remove(key);
            debug_assert!(removed.is_some());
        }
    }

    fn coalesce_prepared(&mut self, candidates : &[usize]) {
        for &candidate in candidates {
            let key = if self.inner
                             .contains_key(&candidate)
            {
                Some(candidate)
            } else {
                self.inner
                    .range(..candidate)
                    .next_back()
                    .map(|(&key, _)| key)
            };
            if let Some(key) = key {
                self.coalesce_at(key);
            }
        }
        self.debug_assert_invariants();
    }

    fn lookup_vma(&self, page : VirtAddr) -> Option<&VmArea> {
        self.lookup(page)
            .and_then(|key| self.inner.get(&key))
    }

    fn first_overlap(&self, start : VirtAddr, end : VirtAddr) -> Option<&VmArea> {
        self.first_overlap_key(start, end)
            .and_then(|key| self.inner.get(&key))
    }

    fn range_from(&self, first : usize, end : VirtAddr) -> impl Iterator<Item = &VmArea> {
        self.inner
            .range(first..end.0)
            .map(|(_, vma)| vma)
    }

    fn range_from_unbounded(&self, first : usize) -> impl Iterator<Item = &VmArea> {
        self.inner
            .range(first..)
            .map(|(_, vma)| vma)
    }

    fn prepare_updated_fragments<F>(&self,
                                    keys : &[usize],
                                    start : VirtAddr,
                                    end : VirtAddr,
                                    mut update : F)
                                    -> MmResult<Vec<VmArea>>
        where F : FnMut(PagePerm) -> PagePerm
    {
        let mut fragments = Vec::new();
        for key in keys {
            let vma = self.inner
                          .get(key)
                          .ok_or(MmError::InvalidAddress)?;
            let mid_start = VirtAddr(core::cmp::max(start.0, vma.start.0));
            let mid_end = VirtAddr(core::cmp::min(end.0, vma.end.0));
            if vma.start.0 < mid_start.0 {
                fragments.push(vma.duplicate_range(vma.start, mid_start)?);
            }
            let mut middle = vma.duplicate_range(mid_start, mid_end)?;
            middle.perm = update(middle.perm);
            fragments.push(middle);
            if mid_end.0 < vma.end.0 {
                fragments.push(vma.duplicate_range(mid_end, vma.end)?);
            }
        }
        Ok(fragments)
    }

    fn commit_replacement(&mut self, keys : &[usize], fragments : Vec<VmArea>) {
        self.remove_prepared(keys);
        let candidates : Vec<usize> = fragments.iter()
                                               .map(|vma| vma.start.0)
                                               .collect();
        for fragment in fragments {
            self.insert_prepared(fragment);
        }
        self.coalesce_prepared(&candidates);
    }

    /// 复制完全落在同一个 VMA 内的子区间，并同步修正文件/设备 backing 的起点。
    pub fn duplicate_subrange(&self, start : VirtAddr, end : VirtAddr) -> MmResult<Option<VmArea>> {
        validate_page_range(start, end)?;
        let Some(vma) = self.lookup_vma(start) else {
            return Ok(None);
        };
        if end.0 > vma.end.0 {
            return Ok(None);
        }
        vma.duplicate_range(start, end)
           .map(Some)
    }

    pub fn overlaps(&self, start : VirtAddr, end : VirtAddr) -> bool {
        self.overlap_end(start, end)
            .is_some()
    }

    pub fn overlaps_where<P>(&self, start : VirtAddr, end : VirtAddr, mut predicate : P) -> bool
        where P : FnMut(&VmArea) -> bool {
        let Some(first) = self.first_overlap_key(start, end) else {
            return false;
        };
        self.range_from(first, end)
            .any(|vma| predicate(vma))
    }

    pub fn overlap_end(&self, start : VirtAddr, end : VirtAddr) -> Option<VirtAddr> {
        if start.0 >= end.0 {
            return None;
        }
        self.first_overlap(start, end)
            .map(|vma| vma.end)
    }

    /// 判断半开区间是否逐字节由相邻 VMA 完整覆盖。
    pub fn covers(&self, start : VirtAddr, end : VirtAddr) -> bool {
        self.covers_where(start, end, |_| true)
    }

    /// 判断半开区间是否由满足 `predicate` 的相邻 VMA 完整覆盖。
    pub fn covers_where<P>(&self, start : VirtAddr, end : VirtAddr, mut predicate : P) -> bool
        where P : FnMut(&VmArea) -> bool {
        if start.0 >= end.0 {
            return true;
        }
        let mut cursor = start.0;
        let Some(first) = self.first_overlap_key(start, end) else {
            return false;
        };
        let mut values = self.range_from_unbounded(first);
        while cursor < end.0 {
            let Some(vma) = values.next() else {
                return false;
            };
            if vma.start.0 > cursor || vma.end.0 <= cursor || !predicate(vma) {
                return false;
            }
            cursor = core::cmp::min(vma.end.0, end.0);
        }
        true
    }

    /// 删除区间并保留左右片段。所有可能失败的 backing 复制在提交前完成。
    pub fn remove_range(&mut self, start : VirtAddr, end : VirtAddr) -> MmResult<()> {
        if start.0 >= end.0 {
            return Ok(());
        }
        validate_page_range(start, end)?;
        let keys = self.overlapping_keys(start, end);
        let mut fragments = Vec::new();
        for key in &keys {
            let vma = self.inner
                          .get(key)
                          .ok_or(MmError::InvalidAddress)?;
            if vma.start.0 < start.0 {
                fragments.push(vma.duplicate_range(vma.start, start)?);
            }
            if end.0 < vma.end.0 {
                fragments.push(vma.duplicate_range(end, vma.end)?);
            }
        }
        self.commit_replacement(&keys, fragments);
        Ok(())
    }

    /// 替换区间内所有 VMA 的权限；跨洞范围由调用方在提交前用 [`Self::covers`] 拒绝。
    pub fn protect_range(&mut self,
                         start : VirtAddr,
                         end : VirtAddr,
                         perm : PagePerm)
                         -> MmResult<()> {
        self.update_perm(start, end, |_| perm)
    }

    /// 将区间内权限按位合并，供 ELF 中落入同一页的 `PT_LOAD` 段使用。
    pub fn merge_perm(&mut self,
                      start : VirtAddr,
                      end : VirtAddr,
                      perm : PagePerm)
                      -> MmResult<()> {
        self.update_perm(start, end, |old| old | perm)
    }

    fn update_perm<F>(&mut self, start : VirtAddr, end : VirtAddr, mut update : F) -> MmResult<()>
        where F : FnMut(PagePerm) -> PagePerm {
        if start.0 >= end.0 {
            return Ok(());
        }
        validate_page_range(start, end)?;
        let keys = self.overlapping_keys(start, end);
        let fragments = self.prepare_updated_fragments(&keys, start, end, &mut update)?;
        self.commit_replacement(&keys, fragments);
        Ok(())
    }
}

fn validate_page_range(start : VirtAddr, end : VirtAddr) -> MmResult<()> {
    if start.0 >= end.0 || start.0 % PAGE_SIZE != 0 || end.0 % PAGE_SIZE != 0 {
        return Err(MmError::InvalidAddress);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rw() -> PagePerm { PagePerm::R | PagePerm::W | PagePerm::U }

    #[test]
    fn ordered_insert_rejects_overlap_and_coalesces_compatible_ranges() {
        let mut set = VmaSet::new();
        set.insert(VmArea::anonymous(VirtAddr(0x3000),
                                     VirtAddr(0x4000),
                                     rw(),
                                     VmaSharing::Private,
                                     VmaKind::Anonymous,
                                     true))
           .unwrap();
        set.insert(VmArea::anonymous(VirtAddr(0x1000),
                                     VirtAddr(0x2000),
                                     rw(),
                                     VmaSharing::Private,
                                     VmaKind::Anonymous,
                                     true))
           .unwrap();
        set.insert(VmArea::anonymous(VirtAddr(0x2000),
                                     VirtAddr(0x3000),
                                     rw(),
                                     VmaSharing::Private,
                                     VmaKind::Anonymous,
                                     true))
           .unwrap();
        assert_eq!(set.len(), 1);
        assert_eq!(set.lookup(VirtAddr(0x1000))
                      .and_then(|key| set.get(key))
                      .unwrap()
                      .start,
                   VirtAddr(0x1000));
        assert_eq!(set.lookup(VirtAddr(0x3000))
                      .and_then(|key| set.get(key))
                      .unwrap()
                      .end,
                   VirtAddr(0x4000));
        assert!(set.insert(VmArea::anonymous(VirtAddr(0x2000),
                                             VirtAddr(0x5000),
                                             rw(),
                                             VmaSharing::Private,
                                             VmaKind::Anonymous,
                                             true))
                   .is_err());
    }

    #[test]
    fn remove_and_protect_split_one_canonical_vma() {
        let mut set = VmaSet::new();
        set.insert(VmArea::anonymous(VirtAddr(0x1000),
                                     VirtAddr(0x5000),
                                     rw(),
                                     VmaSharing::Private,
                                     VmaKind::Anonymous,
                                     true))
           .unwrap();
        set.protect_range(VirtAddr(0x2000),
                          VirtAddr(0x4000),
                          PagePerm::R | PagePerm::U)
           .unwrap();
        assert_eq!(set.len(), 3);
        assert_eq!(set.lookup(VirtAddr(0x3000))
                      .and_then(|i| set.get(i))
                      .unwrap()
                      .perm,
                   PagePerm::R | PagePerm::U);
        assert!(set.remove_range(VirtAddr(0x2800), VirtAddr(0x4800))
                   .is_err());
        set.remove_range(VirtAddr(0x2000), VirtAddr(0x4000))
           .unwrap();
        assert!(!set.covers(VirtAddr(0x3000), VirtAddr(0x4000)));
        assert!(set.lookup(VirtAddr(0x1000))
                   .is_some());
        assert!(set.lookup(VirtAddr(0x4000))
                   .is_some());
    }

    #[test]
    fn device_split_advances_physical_origin() {
        let lease : Arc<dyn DeviceMappingLease> = Arc::new(());
        let mut set = VmaSet::new();
        set.insert(VmArea::device(VirtAddr(0x1000),
                                  VirtAddr(0x5000),
                                  rw(),
                                  PhysPageNum(10),
                                  lease))
           .unwrap();
        set.remove_range(VirtAddr(0x1000), VirtAddr(0x3000))
           .unwrap();
        assert_eq!(set.lookup(VirtAddr(0x3000))
                      .and_then(|key| set.get(key))
                      .unwrap()
                      .device_phys_start(),
                   Some(PhysPageNum(12)));
    }

    #[test]
    fn external_ranges_keep_non_owned_semantics_when_split() {
        let mut set = VmaSet::new();
        set.insert(VmArea::external(VirtAddr(0x1000), VirtAddr(0x5000), rw()))
           .unwrap();
        assert!(set.covers_where(VirtAddr(0x1000),
                                 VirtAddr(0x5000),
                                 VmArea::is_external));
        set.protect_range(VirtAddr(0x2000),
                          VirtAddr(0x4000),
                          PagePerm::R | PagePerm::U)
           .unwrap();
        assert_eq!(set.len(), 3);
        assert!(set.iter().all(VmArea::is_non_owned));
        assert!(!set.covers_where(VirtAddr(0x1000), VirtAddr(0x5000), VmArea::is_device));
    }

    #[test]
    fn file_subrange_and_remove_keep_offsets_consistent() {
        let mut set = VmaSet::new();
        set.insert(VmArea::file(VirtAddr(0x1000),
                                VirtAddr(0x5000),
                                rw(),
                                VmaSharing::Private,
                                false,
                                0x8000,
                                0x4000,
                                None))
           .unwrap();
        let subrange = set.duplicate_subrange(VirtAddr(0x2000), VirtAddr(0x4000))
                          .unwrap()
                          .unwrap();
        assert_eq!(subrange.file_offset, 0x9000);
        set.remove_range(VirtAddr(0x2000), VirtAddr(0x3000))
           .unwrap();
        let right = set.lookup(VirtAddr(0x3000))
                       .and_then(|index| set.get(index))
                       .unwrap();
        assert_eq!(right.file_offset, 0xA000);
    }
}

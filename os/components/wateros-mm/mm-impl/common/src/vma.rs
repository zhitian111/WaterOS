//! 双架构共享的 VMA 表示。
//!
//! 注意：本模块属于 `mm-impl-common`，不是稳定 API；仅用于消除 Sv39 /
//! LoongArch64 中的重复定义。后续 Task 02 会继续把维护操作收口到注册表。

use alloc::boxed::Box;
use alloc::sync::Arc;
use alloc::vec::Vec;

use api_v0::addr::{PhysPageNum, VirtAddr};
use api_v0::error::{MmError, MmResult};
use api_v0::mmap::{DemandPageLoader, DeviceMappingLease};
use api_v0::perm::PagePerm;

pub struct LazyFileVma {
    pub start : VirtAddr,
    pub end : VirtAddr,
    pub perm : PagePerm,
    pub file_offset : usize,
    pub file_size : usize,
    pub loader : Box<dyn DemandPageLoader>,
}

impl LazyFileVma {
    pub fn duplicate(&self) -> MmResult<Self> {
        Ok(Self { start : self.start,
                  end : self.end,
                  perm : self.perm,
                  file_offset : self.file_offset,
                  file_size : self.file_size,
                  loader : self.loader.duplicate_box()? })
    }

    pub fn contains_page(&self, page : VirtAddr) -> bool {
        page.0 >= self.start.0 && page.0 < self.end.0
    }

    pub fn overlaps(&self, start : VirtAddr, end : VirtAddr) -> bool {
        start.0 < self.end.0 && end.0 > self.start.0
    }
}

#[derive(Clone, Copy)]
pub struct SharedAnonVma {
    pub start : VirtAddr,
    pub end : VirtAddr,
}

impl SharedAnonVma {
    pub fn contains_page(&self, page : VirtAddr) -> bool {
        page.0 >= self.start.0 && page.0 < self.end.0
    }

    pub fn overlaps(&self, start : VirtAddr, end : VirtAddr) -> bool {
        start.0 < self.end.0 && end.0 > self.start.0
    }
}

pub struct SharedFileVma {
    pub start : VirtAddr,
    pub end : VirtAddr,
    pub file_offset : usize,
    pub loader : Box<dyn DemandPageLoader>,
}

impl SharedFileVma {
    pub fn duplicate(&self) -> MmResult<Self> {
        Ok(Self { start : self.start,
                  end : self.end,
                  file_offset : self.file_offset,
                  loader : self.loader.duplicate_box()? })
    }

    pub fn overlaps(&self, start : VirtAddr, end : VirtAddr) -> bool {
        start.0 < self.end.0 && end.0 > self.start.0
    }
}

#[derive(Clone)]
pub struct DeviceVma {
    pub start : VirtAddr,
    pub end : VirtAddr,
    pub phys_start : PhysPageNum,
    pub perm : PagePerm,
    pub lease : Arc<dyn DeviceMappingLease>,
}

impl DeviceVma {
    pub fn contains_page(&self, page : VirtAddr) -> bool {
        page.0 >= self.start.0 && page.0 < self.end.0
    }

    pub fn overlaps(&self, start : VirtAddr, end : VirtAddr) -> bool {
        start.0 < self.end.0 && end.0 > self.start.0
    }
}

/// 有序、无重叠的 lazy file VMA 集合。
///
/// 所有修改方法都会在结束时重新建立有序性；查找先走二分，必要时线性回退，
/// 避免调用方因为列表短暂失序而漏页。
pub struct LazyVmaSet {
    inner : Vec<LazyFileVma>,
}

impl LazyVmaSet {
    pub fn new() -> Self { Self { inner : Vec::new() } }

    pub fn from_vec(inner : Vec<LazyFileVma>) -> Self {
        let mut set = Self { inner };
        set.rebuild_order();
        set
    }

    pub fn iter(&self) -> core::slice::Iter<'_, LazyFileVma> { self.inner.iter() }

    pub fn iter_mut(&mut self) -> core::slice::IterMut<'_, LazyFileVma> {
        self.inner.iter_mut()
    }

    pub fn get(&self, index : usize) -> Option<&LazyFileVma> { self.inner.get(index) }

    pub fn get_mut(&mut self, index : usize) -> Option<&mut LazyFileVma> {
        self.inner.get_mut(index)
    }

    pub fn len(&self) -> usize { self.inner.len() }

    pub fn clear(&mut self) { self.inner.clear(); }

    pub fn take(&mut self) -> Vec<LazyFileVma> { core::mem::take(&mut self.inner) }

    pub fn replace(&mut self, mut next : Vec<LazyFileVma>) {
        next.sort_by_key(|vma| vma.start);
        self.inner = next;
    }

    pub fn partition_point<P>(&self, mut pred : P) -> usize
        where P : FnMut(&LazyFileVma) -> bool
    {
        self.inner.partition_point(|vma| pred(vma))
    }

    pub fn insert(&mut self, index : usize, vma : LazyFileVma) {
        self.inner.insert(index, vma);
    }

    pub fn sort(&mut self) { self.rebuild_order(); }

    pub fn lookup(&self, page : VirtAddr) -> Option<usize> {
        let mut low = 0usize;
        let mut high = self.inner.len();
        while low < high {
            let mid = low + (high - low) / 2;
            if self.inner[mid].end.0 <= page.0 {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        if let Some(vma) = self.inner.get(low) {
            if vma.contains_page(page) {
                return Some(low);
            }
        }
        self.inner
            .iter()
            .position(|vma| vma.contains_page(page))
    }

    pub fn overlaps(&self, start : VirtAddr, end : VirtAddr) -> bool {
        if start.0 >= end.0 {
            return false;
        }
        let index = self.partition_point(|vma| vma.end.0 <= start.0);
        self.inner
            .get(index)
            .is_some_and(|vma| vma.start.0 < end.0)
    }

    pub fn overlap_end(&self, start : VirtAddr, end : VirtAddr) -> Option<VirtAddr> {
        self.lookup_or_overlap_end(start, end)
    }

    pub fn merge_perm(&mut self,
                      start : VirtAddr,
                      end : VirtAddr,
                      perm : PagePerm)
                      -> MmResult<()> {
        let mut next = Vec::new();
        for vma in self.inner.drain(..) {
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
                                    perm : vma.perm | perm,
                                    file_offset : vma.file_offset +
                                                  (mid_start.0 - vma.start.0),
                                    file_size : vma.file_size,
                                    loader : vma.loader.duplicate_box()? });
            if end.0 < vma.end.0 {
                next.push(LazyFileVma { start : end,
                                        end : vma.end,
                                        perm : vma.perm,
                                        file_offset : vma.file_offset +
                                                      (end.0 - vma.start.0),
                                        file_size : vma.file_size,
                                        loader : vma.loader });
            }
        }
        self.replace(next);
        Ok(())
    }

    pub fn remove_range(&mut self, start : VirtAddr, end : VirtAddr) -> MmResult<()> {
        let mut next = Vec::new();
        for vma in self.inner.drain(..) {
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
                                        file_offset : vma.file_offset
                                                         .saturating_add(delta),
                                        file_size : vma.file_size,
                                        loader : vma.loader });
            }
        }
        self.replace(next);
        Ok(())
    }

    pub fn protect_range(&mut self,
                         start : VirtAddr,
                         end : VirtAddr,
                         perm : PagePerm)
                         -> MmResult<()> {
        if start.0 >= end.0 {
            return Ok(());
        }
        let first = self.partition_point(|vma| vma.end.0 <= start.0);
        let last = self.partition_point(|vma| vma.start.0 < end.0);
        if first >= last {
            return Ok(());
        }
        let first_vma = &self.inner[first];
        let split_left = (start.0 > first_vma.start.0).then(|| {
            Ok::<_, MmError>(LazyFileVma { start : first_vma.start,
                                           end : start,
                                           perm : first_vma.perm,
                                           file_offset : first_vma.file_offset,
                                           file_size : first_vma.file_size,
                                           loader : first_vma.loader.duplicate_box()? })
        }).transpose()?;
        let last_vma = &self.inner[last - 1];
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
            let first_vma = &mut self.inner[first];
            first_vma.file_offset += start.0 - first_vma.start.0;
            first_vma.start = start;
        }
        if split_right.is_some() {
            self.inner[last - 1].end = end;
        }
        for vma in &mut self.inner[first..last] {
            vma.perm = perm;
        }
        if let Some(right) = split_right {
            self.inner.insert(last, right);
        }
        if let Some(left) = split_left {
            self.inner.insert(first, left);
        }
        self.rebuild_order();
        Ok(())
    }

    fn lookup_or_overlap_end(&self,
                             start : VirtAddr,
                             end : VirtAddr)
                             -> Option<VirtAddr> {
        let mut low = 0usize;
        let mut high = self.inner.len();
        while low < high {
            let mid = low + (high - low) / 2;
            if self.inner[mid].end.0 <= start.0 {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        if let Some(vma) = self.inner.get(low) {
            if vma.overlaps(start, end) {
                return Some(vma.end);
            }
        }
        self.inner
            .iter()
            .find(|vma| vma.overlaps(start, end))
            .map(|vma| vma.end)
    }

    fn rebuild_order(&mut self) {
        self.inner.sort_by_key(|vma| vma.start);
    }
}

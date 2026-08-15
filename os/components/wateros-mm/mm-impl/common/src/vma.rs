//! 双架构共享的 VMA 表示。
//!
//! 注意：本模块属于 `mm-impl-common`，不是稳定 API；仅用于消除 Sv39 /
//! LoongArch64 中的重复定义。后续 Task 02 会继续把维护操作收口到注册表。

use alloc::boxed::Box;
use alloc::sync::Arc;

use api_v0::addr::{PhysPageNum, VirtAddr};
use api_v0::error::MmResult;
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

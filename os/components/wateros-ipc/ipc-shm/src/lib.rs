#![no_std]
//! SysV shared-memory object registry.
//!
//! This crate owns segment identity and physical-frame lifetime. Syscall code is
//! responsible for Linux ABI decoding and mapping returned PPNs into a concrete
//! user address space.

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use api_v0::addr::{PhysPageNum, PAGE_SIZE};
use frame_alloctor::{frame_alloc_result, frame_dealloc_result};
use spin::Mutex;

/// Linux `IPC_PRIVATE`.
pub const IPC_PRIVATE: usize = 0;
/// Linux `IPC_CREAT`.
pub const IPC_CREAT: usize = 0o1000;
/// Linux `IPC_EXCL`.
pub const IPC_EXCL: usize = 0o2000;
/// Linux `SHM_RDONLY`.
pub const SHM_RDONLY: usize = 0o10000;

/// Bring-up limit for one shared segment. Move to base-config once policy grows.
pub const MAX_SHM_SEGMENT_SIZE: usize = 4 * 1024 * 1024;

pub type ShmId = usize;
pub type TaskId = usize;
pub type ShmResult<T> = Result<T, ShmError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShmError {
    Invalid,
    Exists,
    NoEntry,
    NoMem,
    NoSys,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShmSegmentInfo {
    pub shmid: ShmId,
    pub key: usize,
    pub size: usize,
    pub mode: usize,
    pub pages: Vec<PhysPageNum>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShmAttachInfo {
    pub shmid: ShmId,
    pub base: usize,
    pub size: usize,
    pub readonly: bool,
    pub pages: Vec<PhysPageNum>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ShmAttachment {
    shmid: ShmId,
    base: usize,
    size: usize,
    readonly: bool,
}

#[derive(Debug)]
struct ShmSegment {
    key: usize,
    size: usize,
    mode: usize,
    pages: Vec<PhysPageNum>,
    nattch: usize,
    marked_removed: bool,
}

pub struct ShmRegistry {
    next_id: ShmId,
    segments: BTreeMap<ShmId, ShmSegment>,
    key_index: BTreeMap<usize, ShmId>,
    attachments: BTreeMap<TaskId, Vec<ShmAttachment>>,
}

static SHM_REGISTRY: Mutex<ShmRegistry> = Mutex::new(ShmRegistry::new());

pub fn registry() -> &'static Mutex<ShmRegistry> {
    &SHM_REGISTRY
}

impl ShmRegistry {
    pub const fn new() -> Self {
        Self {
            next_id: 1,
            segments: BTreeMap::new(),
            key_index: BTreeMap::new(),
            attachments: BTreeMap::new(),
        }
    }

    pub fn create_or_get(&mut self, key: usize, size: usize, flags: usize) -> ShmResult<ShmId> {
        if size == 0 || size > MAX_SHM_SEGMENT_SIZE {
            return Err(ShmError::Invalid);
        }
        if key != IPC_PRIVATE {
            if let Some(shmid) = self.key_index.get(&key).copied() {
                if flags & IPC_CREAT != 0 && flags & IPC_EXCL != 0 {
                    return Err(ShmError::Exists);
                }
                return Ok(shmid);
            }
            if flags & IPC_CREAT == 0 {
                return Err(ShmError::NoEntry);
            }
        }

        let shmid = self.alloc_id()?;
        let pages = alloc_segment_pages(size)?;
        let segment = ShmSegment {
            key,
            size: round_up_pages(size)?,
            mode: flags & 0o777,
            pages,
            nattch: 0,
            marked_removed: false,
        };
        if key != IPC_PRIVATE {
            self.key_index.insert(key, shmid);
        }
        self.segments.insert(shmid, segment);
        Ok(shmid)
    }

    pub fn segment_info(&self, shmid: ShmId) -> ShmResult<ShmSegmentInfo> {
        let segment = self.segments.get(&shmid).ok_or(ShmError::Invalid)?;
        Ok(ShmSegmentInfo {
            shmid,
            key: segment.key,
            size: segment.size,
            mode: segment.mode,
            pages: segment.pages.clone(),
        })
    }

    pub fn attach(
        &mut self,
        shmid: ShmId,
        task_id: TaskId,
        base: usize,
        readonly: bool,
    ) -> ShmResult<ShmAttachInfo> {
        let segment = self.segments.get_mut(&shmid).ok_or(ShmError::Invalid)?;
        let info = ShmAttachInfo {
            shmid,
            base,
            size: segment.size,
            readonly,
            pages: segment.pages.clone(),
        };
        segment.nattch = segment.nattch.checked_add(1).ok_or(ShmError::Invalid)?;
        self.attachments
            .entry(task_id)
            .or_insert_with(Vec::new)
            .push(ShmAttachment {
                shmid,
                base,
                size: segment.size,
                readonly,
            });
        Ok(info)
    }

    pub fn detach(&mut self, task_id: TaskId, base: usize) -> ShmResult<ShmAttachInfo> {
        let list = self.attachments.get_mut(&task_id).ok_or(ShmError::Invalid)?;
        let index = list
            .iter()
            .position(|attach| attach.base == base)
            .ok_or(ShmError::Invalid)?;
        let attach = list.remove(index);
        if list.is_empty() {
            self.attachments.remove(&task_id);
        }
        self.detach_attachment(attach)
    }

    pub fn mark_removed(&mut self, shmid: ShmId) -> ShmResult<()> {
        let remove_now = {
            let segment = self.segments.get_mut(&shmid).ok_or(ShmError::Invalid)?;
            segment.marked_removed = true;
            if segment.key != IPC_PRIVATE {
                self.key_index.remove(&segment.key);
            }
            segment.nattch == 0
        };
        if remove_now {
            self.remove_segment(shmid);
        }
        Ok(())
    }

    pub fn drop_task(&mut self, task_id: TaskId) -> Vec<ShmAttachInfo> {
        let Some(list) = self.attachments.remove(&task_id) else {
            return Vec::new();
        };
        let mut detached = Vec::new();
        for attach in list {
            if let Ok(info) = self.detach_attachment(attach) {
                detached.push(info);
            }
        }
        detached
    }

    pub fn fork_task(&mut self, parent: TaskId, child: TaskId) -> Vec<ShmAttachInfo> {
        let parent_attaches = self.attachments.get(&parent).cloned().unwrap_or_default();
        let mut child_attaches = Vec::new();
        for attach in parent_attaches {
            let Some(segment) = self.segments.get_mut(&attach.shmid) else {
                continue;
            };
            if segment.nattch == usize::MAX {
                continue;
            }
            segment.nattch += 1;
            self.attachments
                .entry(child)
                .or_insert_with(Vec::new)
                .push(attach);
            child_attaches.push(ShmAttachInfo {
                shmid: attach.shmid,
                base: attach.base,
                size: attach.size,
                readonly: attach.readonly,
                pages: segment.pages.clone(),
            });
        }
        child_attaches
    }

    fn alloc_id(&mut self) -> ShmResult<ShmId> {
        for _ in 0..usize::MAX {
            let id = self.next_id;
            self.next_id = self.next_id.wrapping_add(1);
            if self.next_id == 0 {
                self.next_id = 1;
            }
            if !self.segments.contains_key(&id) {
                return Ok(id);
            }
        }
        Err(ShmError::NoMem)
    }

    fn detach_attachment(&mut self, attach: ShmAttachment) -> ShmResult<ShmAttachInfo> {
        let (info, remove_now) = {
            let segment = self.segments.get_mut(&attach.shmid).ok_or(ShmError::Invalid)?;
            let info = ShmAttachInfo {
                shmid: attach.shmid,
                base: attach.base,
                size: attach.size,
                readonly: attach.readonly,
                pages: segment.pages.clone(),
            };
            if segment.nattch > 0 {
                segment.nattch -= 1;
            }
            (info, segment.marked_removed && segment.nattch == 0)
        };
        if remove_now {
            self.remove_segment(attach.shmid);
        }
        Ok(info)
    }

    fn remove_segment(&mut self, shmid: ShmId) {
        if let Some(segment) = self.segments.remove(&shmid) {
            if segment.key != IPC_PRIVATE {
                self.key_index.remove(&segment.key);
            }
            for page in segment.pages {
                let _ = frame_dealloc_result(page);
            }
        }
    }
}

fn round_up_pages(size: usize) -> ShmResult<usize> {
    size.checked_add(PAGE_SIZE - 1)
        .map(|v| v / PAGE_SIZE * PAGE_SIZE)
        .ok_or(ShmError::Invalid)
}

fn alloc_segment_pages(size: usize) -> ShmResult<Vec<PhysPageNum>> {
    let rounded = round_up_pages(size)?;
    let count = rounded / PAGE_SIZE;
    let mut pages = Vec::new();
    for _ in 0..count {
        let page = match frame_alloc_result() {
            Ok(page) => page,
            Err(_) => {
                for allocated in pages {
                    let _ = frame_dealloc_result(allocated);
                }
                return Err(ShmError::NoMem);
            }
        };
        zero_page(page);
        pages.push(page);
    }
    Ok(pages)
}

fn zero_page(page: PhysPageNum) {
    let addr = page.start_addr().0 as *mut u8;
    unsafe {
        core::ptr::write_bytes(addr, 0, PAGE_SIZE);
    }
}

#![no_std]
//! SysV 共享内存段注册表。
//!
//! 本 crate 管理段标识与物理帧生命周期；syscall 层负责 Linux ABI 解码，并将返回的 PPN 映射到具体用户地址空间。

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::vec::Vec;

use api_v0::addr::{PhysPageNum, PAGE_SIZE};
use frame_alloctor::{frame_alloc_result, frame_dealloc_result};
use spin::Mutex;

/// Linux `IPC_PRIVATE` 键。
pub const IPC_PRIVATE: usize = 0;
/// Linux `IPC_CREAT` 标志。
pub const IPC_CREAT: usize = 0o1000;
/// Linux `IPC_EXCL` 标志。
pub const IPC_EXCL: usize = 0o2000;
/// Linux `SHM_RDONLY` 附加标志。
pub const SHM_RDONLY: usize = 0o10000;

/// bring-up 阶段单段大小上限；策略成熟后迁至 `base-config`。
pub const MAX_SHM_SEGMENT_SIZE: usize = 4 * 1024 * 1024;

/// 共享内存段标识符。
pub type ShmId = usize;
/// 任务标识（与 syscall 层 task id 对齐）。
pub type TaskId = usize;
/// 共享内存操作结果。
pub type ShmResult<T> = Result<T, ShmError>;

/// 共享内存操作错误（syscall 层映射为 errno）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShmError {
    /// 参数非法（`EINVAL`）。
    Invalid,
    /// `IPC_CREAT | IPC_EXCL` 时键已存在（`EEXIST`）。
    Exists,
    /// 键不存在且未指定 `IPC_CREAT`（`ENOENT`）。
    NoEntry,
    /// 物理帧分配失败（`ENOMEM`）。
    NoMem,
    /// 操作尚未支持（`ENOSYS`）。
    NoSys,
}

/// 段元数据快照（供 `shmctl(IPC_STAT)` 等路径使用）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShmSegmentInfo {
    /// 段 id。
    pub shmid: ShmId,
    /// SysV 键；`IPC_PRIVATE` 时为 0。
    pub key: usize,
    /// 段大小（页对齐后）。
    pub size: usize,
    /// 创建时 mode 低 9 位。
    pub mode: usize,
    /// 段占用的物理页列表。
    pub pages: Vec<PhysPageNum>,
}

/// 任务附加段信息（供 MM 映射/解除映射）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShmAttachInfo {
    /// 段 id。
    pub shmid: ShmId,
    /// 用户映射基址。
    pub base: usize,
    /// 映射大小。
    pub size: usize,
    /// 是否只读附加。
    pub readonly: bool,
    /// 段物理页列表。
    pub pages: Vec<PhysPageNum>,
}

/// 单任务对某段的附加记录（内部）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ShmAttachment {
    shmid: ShmId,
    base: usize,
    size: usize,
    readonly: bool,
}

/// 内核侧共享内存段状态（内部）。
#[derive(Debug)]
struct ShmSegment {
    key: usize,
    size: usize,
    mode: usize,
    pages: Vec<PhysPageNum>,
    /// 当前附加计数；归零且已 `IPC_RMID` 时释放物理页。
    nattch: usize,
    marked_removed: bool,
}

/// 全局 SysV 共享内存注册表。
pub struct ShmRegistry {
    next_id: ShmId,
    segments: BTreeMap<ShmId, ShmSegment>,
    key_index: BTreeMap<usize, ShmId>,
    attachments: BTreeMap<TaskId, Vec<ShmAttachment>>,
}

static SHM_REGISTRY: Mutex<ShmRegistry> = Mutex::new(ShmRegistry::new());

/// 返回全局共享内存注册表单例。
pub fn registry() -> &'static Mutex<ShmRegistry> {
    &SHM_REGISTRY
}

impl ShmRegistry {
    /// 创建空注册表。
    pub const fn new() -> Self {
        Self {
            next_id: 1,
            segments: BTreeMap::new(),
            key_index: BTreeMap::new(),
            attachments: BTreeMap::new(),
        }
    }

    /// 创建或查找共享内存段（`shmget` 语义子集）。
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

    /// 查询段元数据。
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

    /// 附加段：递增 `nattch` 并登记附件元数据（`shmat` 路径）。
    pub fn attach(
        &mut self,
        shmid: ShmId,
        task_id: TaskId,
        base: usize,
        readonly: bool,
    ) -> ShmResult<ShmAttachInfo> {
        let _ = self.begin_attach(shmid)?;
        self.finish_attach(shmid, task_id, base, readonly)
    }

    /// 在 MM 映射前递增 `nattch`，防止并发 `IPC_RMID`/`shmdt` 释放物理页。
    pub fn begin_attach(&mut self, shmid: ShmId) -> ShmResult<ShmSegmentInfo> {
        let segment = self.segments.get_mut(&shmid).ok_or(ShmError::Invalid)?;
        segment.nattch = segment.nattch.checked_add(1).ok_or(ShmError::Invalid)?;
        Ok(ShmSegmentInfo {
            shmid,
            key: segment.key,
            size: segment.size,
            mode: segment.mode,
            pages: segment.pages.clone(),
        })
    }

    /// 在 `begin_attach` 之后登记附件元数据（不再递增 `nattch`）。
    pub fn finish_attach(
        &mut self,
        shmid: ShmId,
        task_id: TaskId,
        base: usize,
        readonly: bool,
    ) -> ShmResult<ShmAttachInfo> {
        let segment = self.segments.get(&shmid).ok_or(ShmError::Invalid)?;
        let info = ShmAttachInfo {
            shmid,
            base,
            size: segment.size,
            readonly,
            pages: segment.pages.clone(),
        };
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

    /// MM 映射失败时回滚 `begin_attach` 的 `nattch` 占位。
    pub fn cancel_attach_reservation(&mut self, shmid: ShmId) {
        if let Some(segment) = self.segments.get_mut(&shmid) {
            if segment.nattch > 0 {
                segment.nattch -= 1;
            }
            if segment.marked_removed && segment.nattch == 0 {
                self.remove_segment(shmid);
            }
        }
    }

    /// 解除附加（`shmdt` 语义子集）。
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

    /// 标记段待删除（`IPC_RMID`）；`nattch` 归零时立即回收物理页。
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

    /// 任务退出时解除其全部附加并返回解除信息列表。
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

    /// `fork` 时复制父进程共享内存附加关系。
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

    // 线性探测分配 shmid，避免与已存在段冲突。
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

    // 递减 nattch；若段已 RMID 且无人附加则回收物理页。
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

//! SysV SHM 段、key 与 task attachment 注册表。
//!
//! `ARCH:` 本文件只维护元数据和物理页所有权；用户 VA 预留、页表映射、TLB 刷新均由调用方
//! 在 registry 锁外完成。

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use api_v0::*;
use frame_alloctor::frame_dealloc_result;

use crate::allocation::{alloc_segment_pages, round_up_pages};
use crate::state::{ShmAttachment, ShmSegment};

/// `DATA:` 全局 SysV 共享内存注册表。
///
/// `INVARIANT:` `key_index` 只包含未删除且非 `IPC_PRIVATE` 的段；每个 attachment 对应目标段
/// 的一个 `nattch` 引用。所有字段必须在 global 的 `SHM_REGISTRY` 锁内修改。
pub struct ShmRegistry {
    next_id: ShmId,
    /// 段 ID 到物理页后备段的主索引。
    segments: BTreeMap<ShmId, ShmSegment>,
    /// 非 private SysV key 到当前段 ID 的索引。
    key_index: BTreeMap<usize, ShmId>,
    /// task ID 到该任务的所有 SHM 映射记录。
    attachments: BTreeMap<TaskId, Vec<ShmAttachment>>,
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

    /// `FLOW:` `shmget` 语义子集：按 key 查找，或在允许创建时分配并清零物理帧。
    pub fn create_or_get(&mut self, key: usize, size: usize, flags: usize) -> ShmResult<ShmId> {
        if size == 0 || size > MAX_SHM_SEGMENT_SIZE {
            return Err(ShmError::Invalid);
        }
        if key != IPC_PRIVATE {
            if let Some(shmid) = self
                .key_index
                .get(&key)
                .copied()
            {
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
            self.key_index
                .insert(key, shmid);
        }
        self.segments
            .insert(shmid, segment);
        Ok(shmid)
    }

    /// 返回段元数据快照；快照中的页仍由 registry 所有。
    pub fn segment_info(&self, shmid: ShmId) -> ShmResult<ShmSegmentInfo> {
        let segment = self
            .segments
            .get(&shmid)
            .ok_or(ShmError::Invalid)?;
        Ok(ShmSegmentInfo {
            shmid,
            key: segment.key,
            size: segment.size,
            mode: segment.mode,
            pages: segment
                .pages
                .clone(),
        })
    }

    /// 兼容的单阶段 attach；仅在调用方不需要在 MM 映射前解锁时使用。
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

    /// `FLOW:` `shmat` 的第一阶段，在 MM 映射前保留一份 `nattch`。
    ///
    /// `LOCK:` 返回页快照后调用方必须释放 registry 锁再进入 MM；映射失败必须调用
    /// [`Self::cancel_attach_reservation`]，成功后必须调用 [`Self::finish_attach`]。
    pub fn begin_attach(&mut self, shmid: ShmId) -> ShmResult<ShmSegmentInfo> {
        let segment = self
            .segments
            .get_mut(&shmid)
            .ok_or(ShmError::Invalid)?;
        segment.nattch = segment
            .nattch
            .checked_add(1)
            .ok_or(ShmError::Invalid)?;
        Ok(ShmSegmentInfo {
            shmid,
            key: segment.key,
            size: segment.size,
            mode: segment.mode,
            pages: segment
                .pages
                .clone(),
        })
    }

    /// `FLOW:` `begin_attach` 成功且 MM 映射完成后提交 task attachment；不再增加 `nattch`。
    pub fn finish_attach(
        &mut self,
        shmid: ShmId,
        task_id: TaskId,
        base: usize,
        readonly: bool,
    ) -> ShmResult<ShmAttachInfo> {
        let segment = self
            .segments
            .get(&shmid)
            .ok_or(ShmError::Invalid)?;
        let info = ShmAttachInfo {
            shmid,
            base,
            size: segment.size,
            readonly,
            pages: segment
                .pages
                .clone(),
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

    /// `FLOW:` MM 映射失败时撤销 `begin_attach` 预留；对已不存在段幂等。
    pub fn cancel_attach_reservation(&mut self, shmid: ShmId) {
        let remove_now = if let Some(segment) = self
            .segments
            .get_mut(&shmid)
        {
            segment.nattch = segment
                .nattch
                .saturating_sub(1);
            segment.marked_removed && segment.nattch == 0
        } else {
            false
        };
        if remove_now {
            self.remove_segment(shmid);
        }
    }

    /// `FLOW:` `shmdt`：先删除 attachment 并递减 `nattch`，返回页信息给调用方解除页表映射。
    pub fn detach(&mut self, task_id: TaskId, base: usize) -> ShmResult<ShmAttachInfo> {
        let list = self
            .attachments
            .get_mut(&task_id)
            .ok_or(ShmError::Invalid)?;
        let index = list
            .iter()
            .position(|attach| attach.base == base)
            .ok_or(ShmError::Invalid)?;
        let attach = list.remove(index);
        if list.is_empty() {
            self.attachments
                .remove(&task_id);
        }
        self.detach_attachment(attach)
    }

    /// `FLOW:` `IPC_RMID` 立即去除 key 可见性；最后一个 attachment 消失时才释放帧。
    pub fn mark_removed(&mut self, shmid: ShmId) -> ShmResult<()> {
        let remove_now = {
            let segment = self
                .segments
                .get_mut(&shmid)
                .ok_or(ShmError::Invalid)?;
            segment.marked_removed = true;
            if segment.key != IPC_PRIVATE {
                self.key_index
                    .remove(&segment.key);
            }
            segment.nattch == 0
        };
        if remove_now {
            self.remove_segment(shmid);
        }
        Ok(())
    }

    /// `FLOW:` task 退出时摘除其全部 attachment；调用方随后逐项撤销地址空间映射。
    pub fn drop_task(&mut self, task_id: TaskId) -> Vec<ShmAttachInfo> {
        let Some(list) = self
            .attachments
            .remove(&task_id)
        else {
            return Vec::new();
        };
        list.into_iter()
            .filter_map(|attach| {
                self.detach_attachment(attach)
                    .ok()
            })
            .collect()
    }

    /// `FLOW:` `fork` 复制父 task 的 attachment 关系并增加每段 `nattch`；调用方负责映射子地址空间。
    pub fn fork_task(&mut self, parent: TaskId, child: TaskId) -> Vec<ShmAttachInfo> {
        let parent_attaches = self
            .attachments
            .get(&parent)
            .cloned()
            .unwrap_or_default();
        let mut child_attaches = Vec::new();
        for attach in parent_attaches {
            let Some(segment) = self
                .segments
                .get_mut(&attach.shmid)
            else {
                continue;
            };
            let Some(nattch) = segment
                .nattch
                .checked_add(1)
            else {
                continue;
            };
            segment.nattch = nattch;
            self.attachments
                .entry(child)
                .or_insert_with(Vec::new)
                .push(attach);
            child_attaches.push(ShmAttachInfo {
                shmid: attach.shmid,
                base: attach.base,
                size: attach.size,
                readonly: attach.readonly,
                pages: segment
                    .pages
                    .clone(),
            });
        }
        child_attaches
    }

    /// 线性探测分配未占用 shmid，跳过 0。
    fn alloc_id(&mut self) -> ShmResult<ShmId> {
        for _ in 0..usize::MAX {
            let id = self.next_id;
            self.next_id = self
                .next_id
                .wrapping_add(1);
            if self.next_id == 0 {
                self.next_id = 1;
            }
            if !self
                .segments
                .contains_key(&id)
            {
                return Ok(id);
            }
        }
        Err(ShmError::NoMem)
    }

    /// `INVARIANT:` 每次 attachment 只递减一次；必要时在返回 MM 信息前回收已删除段的帧。
    fn detach_attachment(&mut self, attach: ShmAttachment) -> ShmResult<ShmAttachInfo> {
        let (info, remove_now) = {
            let segment = self
                .segments
                .get_mut(&attach.shmid)
                .ok_or(ShmError::Invalid)?;
            let info = ShmAttachInfo {
                shmid: attach.shmid,
                base: attach.base,
                size: attach.size,
                readonly: attach.readonly,
                pages: segment
                    .pages
                    .clone(),
            };
            segment.nattch = segment
                .nattch
                .saturating_sub(1);
            (
                info,
                segment.marked_removed && segment.nattch == 0,
            )
        };
        if remove_now {
            self.remove_segment(attach.shmid);
        }
        Ok(info)
    }

    /// `LOCK:` 只能在 registry 锁内调用。调用者已经确认没有 attachment 再需要这些帧。
    fn remove_segment(&mut self, shmid: ShmId) {
        if let Some(segment) = self
            .segments
            .remove(&shmid)
        {
            if segment.key != IPC_PRIVATE {
                self.key_index
                    .remove(&segment.key);
            }
            for page in segment.pages {
                let _ = frame_dealloc_result(page);
            }
        }
    }
}

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
    /// 下一次 `shmat` 两阶段操作使用的 reservation ID。
    next_attach_reservation_id: usize,
    /// 段 ID 到物理页后备段的主索引。
    segments: BTreeMap<ShmId, ShmSegment>,
    /// 非 private SysV key 到当前段 ID 的索引。
    key_index: BTreeMap<usize, ShmId>,
    /// task ID 到该任务的所有 SHM 映射记录。
    attachments: BTreeMap<TaskId, Vec<ShmAttachment>>,
    /// 已经增加 `nattch`、但尚未完成 MM 映射提交的 attach reservation。
    attach_reservations: BTreeMap<usize, ShmId>,
}

/// `DATA:` 一次 `shmat` 的不可伪造（对外字段私有）提交凭据。
///
/// 只有 [`ShmRegistry::begin_attach`] 能创建它；调用方必须把同一个 reservation 交给
/// `finish_attach` 或 `cancel_attach_reservation`，不能只凭 `shmid` 结束另一条并发 attach。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShmAttachReservation {
    id: usize,
    shmid: ShmId,
}

impl ShmRegistry {
    /// 创建空注册表。
    pub const fn new() -> Self {
        Self {
            next_id: 1,
            next_attach_reservation_id: 1,
            segments: BTreeMap::new(),
            key_index: BTreeMap::new(),
            attachments: BTreeMap::new(),
            attach_reservations: BTreeMap::new(),
        }
    }

    /// `FLOW:` `shmget` 语义子集：按 key 查找，或在允许创建时分配并清零物理帧。
    pub fn create_or_get(&mut self,
                         key : usize,
                         size : usize,
                         flags : usize,
                         owner_uid : u32,
                         owner_gid : u32)
                         -> ShmResult<ShmId> {
        self.create_or_get_with_metadata(key, size, flags, owner_uid, owner_gid, 0, 0)
    }

    /// `shmget` 完整入口；记录创建进程和创建时间供 `IPC_STAT` 使用。
    pub fn create_or_get_with_metadata(&mut self,
                                       key : usize,
                                       size : usize,
                                       flags : usize,
                                       owner_uid : u32,
                                       owner_gid : u32,
                                       creator_pid : i32,
                                       change_time : i64)
                                       -> ShmResult<ShmId> {
        if key != IPC_PRIVATE {
            if let Some(shmid) = self
                .key_index
                .get(&key)
                .copied()
            {
                if flags & IPC_CREAT != 0 && flags & IPC_EXCL != 0 {
                    return Err(ShmError::Exists);
                }
                if size != 0 && size > self.segments[&shmid].size {
                    return Err(ShmError::Invalid);
                }
                return Ok(shmid);
            }
            if flags & IPC_CREAT == 0 {
                return Err(ShmError::NoEntry);
            }
        }
        if size == 0 || size > MAX_SHM_SEGMENT_SIZE {
            return Err(ShmError::Invalid);
        }

        let shmid = self.alloc_id()?;
        let pages = alloc_segment_pages(size)?;
        let segment = ShmSegment {
            key,
            size: round_up_pages(size)?,
            mode: flags & 0o777,
            owner_uid,
            owner_gid,
            creator_uid : owner_uid,
            creator_gid : owner_gid,
            creator_pid,
            last_pid : creator_pid,
            attach_time : 0,
            detach_time : 0,
            change_time,
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
            owner_uid : segment.owner_uid,
            owner_gid : segment.owner_gid,
            creator_uid : segment.creator_uid,
            creator_gid : segment.creator_gid,
            nattch : segment.nattch,
            marked_removed : segment.marked_removed,
            creator_pid : segment.creator_pid,
            last_pid : segment.last_pid,
            attach_time : segment.attach_time,
            detach_time : segment.detach_time,
            change_time : segment.change_time,
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
        let (reservation, _) = self.begin_attach(shmid)?;
        self.finish_attach(&reservation, task_id, base, readonly)
    }

    /// `FLOW:` `shmat` 的第一阶段，在 MM 映射前保留一份 `nattch`。
    ///
    /// `LOCK:` 返回 reservation 与页快照后调用方必须释放 registry 锁再进入 MM；映射失败必须以
    /// 同一个 reservation 调用 [`Self::cancel_attach_reservation`]，成功后必须交给
    /// [`Self::finish_attach`]。
    pub fn begin_attach(&mut self,
                        shmid: ShmId)
                        -> ShmResult<(ShmAttachReservation, ShmSegmentInfo)> {
        let info = self.segment_info(shmid)?;
        let reservation_id = self.alloc_attach_reservation_id()?;
        let segment = self
            .segments
            .get_mut(&shmid)
            .ok_or(ShmError::Invalid)?;
        segment.nattch = segment
            .nattch
            .checked_add(1)
            .ok_or(ShmError::Invalid)?;
        self.attach_reservations.insert(reservation_id, shmid);
        Ok((ShmAttachReservation { id: reservation_id, shmid }, info))
    }

    /// `FLOW:` `begin_attach` 成功且 MM 映射完成后，以同一个 reservation 提交 task attachment；
    /// 不再增加 `nattch`。
    pub fn finish_attach(
        &mut self,
        reservation: &ShmAttachReservation,
        task_id: TaskId,
        base: usize,
        readonly: bool,
    ) -> ShmResult<ShmAttachInfo> {
        self.finish_attach_with_metadata(reservation, task_id, base, readonly, 0, 0)
    }

    /// 提交映射并记录 Linux `shm_atime/shm_lpid`。
    pub fn finish_attach_with_metadata(&mut self,
                                       reservation : &ShmAttachReservation,
                                       task_id : TaskId,
                                       base : usize,
                                       readonly : bool,
                                       operator_pid : i32,
                                       attach_time : i64)
                                       -> ShmResult<ShmAttachInfo> {
        if !self.has_attach_reservation(reservation) {
            return Err(ShmError::Invalid);
        }
        let shmid = reservation.shmid;
        let segment = self
            .segments
            .get_mut(&shmid)
            .ok_or(ShmError::Invalid)?;
        segment.last_pid = operator_pid;
        segment.attach_time = attach_time;
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
        self.attach_reservations.remove(&reservation.id);
        Ok(info)
    }

    /// `FLOW:` MM 映射失败时撤销指定 `begin_attach` reservation；重复或错配 token 返回 `EINVAL`。
    pub fn cancel_attach_reservation(
        &mut self,
        reservation: &ShmAttachReservation,
    ) -> ShmResult<()> {
        if !self.has_attach_reservation(reservation) {
            return Err(ShmError::Invalid);
        }
        let shmid = reservation.shmid;
        self.attach_reservations.remove(&reservation.id);
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
        Ok(())
    }

    /// 返回一个现有 task attachment 的映射快照，但不改变 `nattch`。
    ///
    /// 调用方据此在 registry 锁外解除页表映射；只有成功解除后才能调用 [`Self::detach`]。
    pub fn attachment_info(&self, task_id: TaskId, base: usize) -> ShmResult<ShmAttachInfo> {
        let attach = self
            .attachments
            .get(&task_id)
            .and_then(|list| list.iter().find(|attach| attach.base == base))
            .copied()
            .ok_or(ShmError::Invalid)?;
        self.attachment_info_from_attachment(attach)
    }

    /// 返回一个 task 当前所有 attachment 的快照，不改变 registry。
    pub fn task_attachments(&self, task_id: TaskId) -> Vec<ShmAttachInfo> {
        self.attachments
            .get(&task_id)
            .into_iter()
            .flatten()
            .filter_map(|attach| self.attachment_info_from_attachment(*attach).ok())
            .collect()
    }

    /// `FLOW:` `shmdt`：先删除 attachment 并递减 `nattch`，返回页信息给调用方解除页表映射。
    pub fn detach(&mut self, task_id: TaskId, base: usize) -> ShmResult<ShmAttachInfo> {
        self.detach_with_metadata(task_id, base, 0, 0)
    }

    /// 解除映射并记录 Linux `shm_dtime/shm_lpid`。
    pub fn detach_with_metadata(&mut self,
                                task_id : TaskId,
                                base : usize,
                                operator_pid : i32,
                                detach_time : i64)
                                -> ShmResult<ShmAttachInfo> {
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
        if let Some(segment) = self.segments.get_mut(&attach.shmid) {
            segment.last_pid = operator_pid;
            segment.detach_time = detach_time;
        }
        self.detach_attachment(attach)
    }

    /// 修改所有者和权限低 9 位；调用方负责 Linux 权限校验。
    pub fn update_permissions(&mut self,
                              shmid : ShmId,
                              owner_uid : u32,
                              owner_gid : u32,
                              mode : usize,
                              change_time : i64)
                              -> ShmResult<()> {
        let segment = self.segments.get_mut(&shmid).ok_or(ShmError::Invalid)?;
        segment.owner_uid = owner_uid;
        segment.owner_gid = owner_gid;
        segment.mode = (segment.mode & !0o777) | (mode & 0o777);
        segment.change_time = change_time;
        Ok(())
    }

    /// 返回 `SHM_INFO` 需要的全局统计；不暴露可变 registry 内部结构。
    pub fn stats(&self) -> ShmRegistryStats {
        ShmRegistryStats {
            segment_count : self.segments.len(),
            total_pages : self.segments.values().map(|segment| segment.pages.len()).sum(),
            attached_count : self.segments.values().map(|segment| segment.nattch).sum(),
            max_id : self.segments.keys().next_back().copied().unwrap_or(0),
        }
    }

    /// 返回当前仍存活的全部段快照，按 shmid 递增排列。
    ///
    /// 主要供 `/proc/sysvipc/shm` 等只读观测接口使用；调用方拿到快照后应立即
    /// 释放 registry 锁，再进行字符串格式化或用户复制。
    pub fn segment_infos(&self) -> Vec<ShmSegmentInfo> {
        self.segments
            .keys()
            .filter_map(|shmid| self.segment_info(*shmid).ok())
            .collect()
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

    fn alloc_attach_reservation_id(&mut self) -> ShmResult<usize> {
        for _ in 0..usize::MAX {
            let id = self.next_attach_reservation_id;
            self.next_attach_reservation_id = self.next_attach_reservation_id.wrapping_add(1);
            if self.next_attach_reservation_id == 0 {
                self.next_attach_reservation_id = 1;
            }
            if id != 0 && !self.attach_reservations.contains_key(&id) {
                return Ok(id);
            }
        }
        Err(ShmError::NoMem)
    }

    fn has_attach_reservation(&self, reservation: &ShmAttachReservation) -> bool {
        self.attach_reservations.get(&reservation.id) == Some(&reservation.shmid)
    }

    fn attachment_info_from_attachment(&self, attach: ShmAttachment) -> ShmResult<ShmAttachInfo> {
        let segment = self.segments.get(&attach.shmid).ok_or(ShmError::Invalid)?;
        Ok(ShmAttachInfo {
            shmid: attach.shmid,
            base: attach.base,
            size: attach.size,
            readonly: attach.readonly,
            pages: segment.pages.clone(),
        })
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
            let page_count = segment.pages.len();
            let mut failed = 0usize;
            for page in segment.pages {
                if frame_dealloc_result(page).is_err() {
                    failed += 1;
                }
            }
            if failed != 0 {
                log::warn!("[shm] segment release encountered already-free frames shmid={} failed={}/{}",
                           shmid,
                           failed,
                           page_count);
            }
        }
    }
}

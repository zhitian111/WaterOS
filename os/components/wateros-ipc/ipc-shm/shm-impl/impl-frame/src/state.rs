//! 仅 registry 使用的 SHM 状态。

use alloc::vec::Vec;
use api_v0::{PhysPageNum, ShmId};

/// `DATA:` 单任务附加记录。它不保存地址空间句柄；MM 映射所有权在 syscall/MM 层。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct ShmAttachment {
    pub(super) shmid: ShmId,
    pub(super) base: usize,
    pub(super) size: usize,
    pub(super) readonly: bool,
}

/// `DATA:` 内核持有的共享段。
///
/// `INVARIANT:` 当 `marked_removed && nattch == 0` 时，段必须从 registry 删除并释放 `pages`。
#[derive(Debug)]
pub(super) struct ShmSegment {
    pub(super) key: usize,
    pub(super) size: usize,
    pub(super) mode: usize,
    pub(super) owner_uid : u32,
    pub(super) owner_gid : u32,
    pub(super) creator_uid : u32,
    pub(super) creator_gid : u32,
    pub(super) creator_pid : i32,
    pub(super) last_pid : i32,
    pub(super) attach_time : i64,
    pub(super) detach_time : i64,
    pub(super) change_time : i64,
    pub(super) pages: Vec<PhysPageNum>,
    /// 已完成或正在进行 MM 映射的 attachment 计数。
    pub(super) nattch: usize,
    pub(super) marked_removed: bool,
}

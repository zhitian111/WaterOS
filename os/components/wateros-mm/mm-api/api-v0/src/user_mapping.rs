//! 用户地址空间映射的只读诊断快照。
//!
//! 该接口服务于 procfs、调试器等观察者。具体页表实现负责在自己的地址空间锁
//! 内汇总页表叶子与 VMA 元数据；观察者不能借此修改映射，也不应长期缓存结果。

extern crate alloc;

use alloc::vec::Vec;

use crate::error::{MmError, MmResult};
use crate::perm::PagePerm;

/// `/proc/<pid>/maps` 可区分的映射来源。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserMappingKind {
    Anonymous,
    File,
    Heap,
    Stack,
    Device,
    KernelTrampoline,
}

/// 一段连续、权限与后备类型一致的用户虚拟地址区间。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserMappingSnapshot {
    pub start : usize,
    pub end : usize,
    pub perm : PagePerm,
    pub shared : bool,
    /// 文件映射中的字节偏移；非文件映射恒为 0。
    pub file_offset : usize,
    /// 当前已经安装叶 PTE 的 4 KiB 页数。
    pub resident_pages : usize,
    pub kind : UserMappingKind,
}

pub type SnapshotUserMappingsFn = fn(usize) -> MmResult<Vec<UserMappingSnapshot>>;

static SNAPSHOT_USER_MAPPINGS: spin::Mutex<Option<SnapshotUserMappingsFn>> =
    spin::Mutex::new(None);

/// 由活动 MM 实现在启动时注册。
pub fn register_snapshot_user_mappings_hook(hook : SnapshotUserMappingsFn) {
    *SNAPSHOT_USER_MAPPINGS.lock() = Some(hook);
}

/// 获取地址空间当前的一次性映射快照。
pub fn snapshot_user_mappings(aspace_ptr : usize) -> MmResult<Vec<UserMappingSnapshot>> {
    if aspace_ptr == 0 {
        return Err(MmError::InvalidAddress);
    }
    let hook = *SNAPSHOT_USER_MAPPINGS.lock();
    hook.ok_or(MmError::Unsupported)?(aspace_ptr)
}

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
    /// 无文件后备的普通匿名 VMA。
    Anonymous,
    /// 文件后备 VMA，`file_offset` 指向本快照起点在文件中的字节位置。
    File,
    /// `brk` 管理的进程堆区，通常可读写且向高地址增长。
    Heap,
    /// 初始或线程用户栈区，通常向低地址增长。
    Stack,
    /// 驱动租约保护的设备页映射，解除时不能归还给通用帧分配器。
    Device,
    /// 用户/内核切换必须保留的 trampoline 映射，普通 `munmap` 不应删除。
    KernelTrampoline,
}

/// 一段连续、权限与后备类型一致的用户虚拟地址区间。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UserMappingSnapshot {
    /// 区间起始虚拟字节地址（含），必须小于 `end` 且应页对齐。
    pub start : usize,
    /// 区间结束虚拟字节地址（不含），不是最后一个可访问字节。
    pub end : usize,
    /// 本区间当前的有效页权限。
    pub perm : PagePerm,
    /// 是否以共享语义映射；仅对文件/共享内存等后备有实际含义。
    pub shared : bool,
    /// 文件映射中的字节偏移；非文件映射恒为 0。
    pub file_offset : usize,
    /// 当前已经安装叶 PTE 的 4 KiB 页数。
    pub resident_pages : usize,
    /// 映射来源分类，供 procfs 渲染和调试使用，不应被当作可写控制接口。
    pub kind : UserMappingKind,
}

/// 活动 MM 注册的快照函数。实参为内核不透明地址空间指针，回调必须在内部完成同步并返回独立副本。
pub type SnapshotUserMappingsFn = fn(usize) -> MmResult<Vec<UserMappingSnapshot>>;

/// 当前快照提供者；未注册时公开接口返回 `Unsupported`，而不是空快照以免伪装为无映射。
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

#![no_std]
//! SysV 共享内存 API v0：稳定的段标识、标志、错误和 MM 交接快照。
//!
//! `ARCH:` 本 crate 不保存全局段状态、不分配物理帧，也不读写页表。syscall 层负责 ABI
//! 转换，具体实现层负责 registry 与帧生命周期。

extern crate alloc;

use alloc::vec::Vec;

pub use mm_api::addr::PhysPageNum;

/// Linux `IPC_PRIVATE` 键。
pub const IPC_PRIVATE: usize = 0;
/// Linux `IPC_CREAT` 标志。
pub const IPC_CREAT: usize = 0o1000;
/// Linux `IPC_EXCL` 标志。
pub const IPC_EXCL: usize = 0o2000;
/// Linux `SHM_RDONLY` 附加标志。
pub const SHM_RDONLY: usize = 0o10000;

/// bring-up 阶段单段大小上限；策略成熟后应迁至 base-config。
pub const MAX_SHM_SEGMENT_SIZE: usize = 4 * 1024 * 1024;

/// SysV 共享内存段标识符。
pub type ShmId = usize;
/// 内核任务标识；它是附加记录 key，不等同于 Linux PID/TID。
pub type TaskId = usize;
/// SHM 领域操作结果；syscall 层负责映射 errno。
pub type ShmResult<T> = Result<T, ShmError>;

/// SHM 领域错误。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShmError {
    /// 参数、段 ID 或生命周期阶段非法（`EINVAL`）。
    Invalid,
    /// `IPC_CREAT | IPC_EXCL` 时键已存在（`EEXIST`）。
    Exists,
    /// 键不存在且未指定 `IPC_CREAT`（`ENOENT`）。
    NoEntry,
    /// 物理帧分配失败或段 ID 空间耗尽（`ENOMEM`）。
    NoMem,
    /// 操作尚未支持（`ENOSYS`）。
    NoSys,
}

/// `DATA:` 段元数据快照，供 `shmctl(IPC_STAT)` 与 `shmat` 的 MM 映射阶段使用。
///
/// `pages` 是实现持有的物理页；调用方只能把它们映射进地址空间，不能自行释放。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShmSegmentInfo {
    /// 内核分配的段标识；删除并重建后不能假定其不变。
    pub shmid: ShmId,
    /// SysV 键；`IPC_PRIVATE` 不进入全局键索引。
    pub key: usize,
    /// 页对齐后的映射长度。
    pub size: usize,
    /// 创建时 mode 低 9 位。
    pub mode: usize,
    /// 创建者的有效 uid/gid，用于 `shmat` 权限判断。
    pub owner_uid : u32,
    pub owner_gid : u32,
    /// 创建者身份不会随 `IPC_SET` 改变。
    pub creator_uid : u32,
    pub creator_gid : u32,
    /// 已完成或正在提交的附加数量。
    pub nattch : usize,
    /// 是否已执行 `IPC_RMID`；已附加任务仍可继续使用直到最后一次 detach。
    pub marked_removed : bool,
    /// 创建此段的进程号。
    pub creator_pid : i32,
    /// 最近 attach/detach/控制操作的进程号。
    pub last_pid : i32,
    /// 最近成功 attach 的时间；单位和时钟来源由 syscall 层定义。
    pub attach_time : i64,
    /// 最近 detach 的时间；单位和时钟来源由 syscall 层定义。
    pub detach_time : i64,
    /// 最近 IPC_SET/创建修改的时间。
    pub change_time : i64,
    /// registry 所有的物理页列表；调用方仅可映射，不得释放。
    pub pages: Vec<PhysPageNum>,
}

/// `DATA:` `shmctl(SHM_INFO)` 所需的全局只读统计。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShmRegistryStats {
    pub segment_count : usize,
    pub total_pages : usize,
    pub attached_count : usize,
    /// 当前实现以 shmid 直接作为可枚举索引，因此返回最大的在用 ID。
    pub max_id : usize,
}

/// `DATA:` 一次任务附加的 MM 交接信息。
///
/// `readonly` 决定页表权限；页的所有权仍属于 SHM registry，直到 `IPC_RMID` 且附加计数为零。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShmAttachInfo {
    /// 所属段 ID。
    pub shmid: ShmId,
    /// 目标地址空间内的映射起始虚拟地址。
    pub base: usize,
    /// 页对齐映射长度（字节）。
    pub size: usize,
    /// 是否禁止用户写入此映射。
    pub readonly: bool,
    pub pages: Vec<PhysPageNum>,
}

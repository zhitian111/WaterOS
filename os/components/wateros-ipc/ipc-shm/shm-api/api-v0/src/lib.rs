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
    pub shmid: ShmId,
    pub key: usize,
    /// 页对齐后的映射长度。
    pub size: usize,
    /// 创建时 mode 低 9 位。
    pub mode: usize,
    pub pages: Vec<PhysPageNum>,
}

/// `DATA:` 一次任务附加的 MM 交接信息。
///
/// `readonly` 决定页表权限；页的所有权仍属于 SHM registry，直到 `IPC_RMID` 且附加计数为零。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShmAttachInfo {
    pub shmid: ShmId,
    pub base: usize,
    pub size: usize,
    pub readonly: bool,
    pub pages: Vec<PhysPageNum>,
}

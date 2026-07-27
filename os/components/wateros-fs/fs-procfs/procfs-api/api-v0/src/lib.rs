#![no_std]

//! procfs 只读视图 API（v0）。

extern crate alloc;

use alloc::{string::String, vec::Vec};

pub use fs_api_v0::{FsDirEntry, FsError, FsMetadata, FsNodeType, FsResult};

/// 与 `task::TaskId` 数值一致，api 层不依赖 task crate。
pub type TaskId = usize;

/// `/proc/mounts` 单行：`device mount_point fstype ...`。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcMountLine {
    /// `mnt_fsname`（第 1 列）。
    pub device: String,
    /// `mnt_dir`（第 2 列）。
    pub mount_point: String,
    /// `mnt_type`（第 3 列）。
    pub fstype: String,
}

/// 按 leader task id 查询 argv。
pub type TaskArgvLookup = fn(TaskId) -> Option<Vec<String>>;

/// 按 leader task id 查询 exe 路径。
pub type TaskExeLookup = fn(TaskId) -> Option<String>;

/// 枚举当前挂载表（供 `/proc/mounts`）。
pub type MountListLookup = fn() -> Vec<ProcMountLine>;

/// 查询内核启动后的单调时长，单位纳秒。
pub type UptimeLookup = fn() -> u128;

/// 查询所有 CPU 聚合 idle 时间，单位纳秒。
pub type IdleTimeLookup = fn() -> u128;

/// procfs 只读路径操作；`rel_path` 为相对 `/proc` 的路径（可带或不带前导 `/`）。
pub trait ProcFsView {
    /// 路径是否对应已知 proc 节点（含目录与文件）。
    fn exists(&self, rel_path: &str) -> FsResult<bool>;
    /// 查询节点元数据；不存在返回 [`FsError::NotFound`]。
    fn metadata(&self, rel_path: &str) -> FsResult<FsMetadata>;
    /// 读取普通文件内容；目录路径返回 [`FsError::NotAFile`]。
    fn read(&self, rel_path: &str) -> FsResult<Vec<u8>>;
    /// 读取符号链接目标；非符号链接返回 [`FsError::NotAFile`]。
    fn read_symlink(&self, rel_path: &str) -> FsResult<Vec<u8>>;
    /// 列出目录项；非目录返回 [`FsError::NotAFile`]。
    fn read_dir(&self, rel_path: &str) -> FsResult<Vec<FsDirEntry>>;
}

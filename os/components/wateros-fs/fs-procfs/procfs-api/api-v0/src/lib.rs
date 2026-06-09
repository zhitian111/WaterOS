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

/// procfs 只读路径操作。
pub trait ProcFsView {
    fn exists(&self, rel_path: &str) -> FsResult<bool>;
    fn metadata(&self, rel_path: &str) -> FsResult<FsMetadata>;
    fn read(&self, rel_path: &str) -> FsResult<Vec<u8>>;
    fn read_dir(&self, rel_path: &str) -> FsResult<Vec<FsDirEntry>>;
}

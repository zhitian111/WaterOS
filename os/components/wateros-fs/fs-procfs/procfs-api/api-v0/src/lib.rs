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
    /// 当前挂载是否只读；用于生成与实际写权限一致的 `/proc/mounts`。
    pub readonly: bool,
}

/// 按 leader task id 查询 argv。
pub type TaskArgvLookup = fn(TaskId) -> Option<Vec<String>>;

/// 按 leader task id 查询 exe 路径。
pub type TaskExeLookup = fn(TaskId) -> Option<String>;

/// 按 task id 枚举当前打开的文件描述符。
pub type TaskFdLookup = fn(TaskId) -> Vec<usize>;

/// 查询一个打开 fd 在 `/proc/<pid>/fd/N` 中应显示的链接目标。
pub type TaskFdTargetLookup = fn(TaskId, usize) -> Option<String>;

/// 按 task id 查询当前 timer slack，单位纳秒。
pub type TaskTimerSlackLookup = fn(TaskId) -> u64;

/// 枚举当前挂载表（供 `/proc/mounts`）。
pub type MountListLookup = fn() -> Vec<ProcMountLine>;

/// 查询内核启动后的单调时长，单位纳秒。
pub type UptimeLookup = fn() -> u128;

/// 查询所有 CPU 聚合 idle 时间，单位纳秒。
pub type IdleTimeLookup = fn() -> u128;

/// `/proc/sysvipc` 中的 Linux SysV IPC 表类型。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SysVIpcTable {
    Shm,
    Msg,
    Sem,
}

/// 查询一张 SysV IPC 文本表；回调方负责从对应注册表生成一致快照。
pub type SysVIpcTableLookup = fn(SysVIpcTable) -> Vec<u8>;

/// procfs 只读路径操作；`rel_path` 为相对 `/proc` 的路径（可带或不带前导 `/`）。
pub trait ProcFsView {
    /// 路径是否对应已知 proc 节点（含目录与文件）。
    fn exists(&self, rel_path: &str) -> FsResult<bool>;
    /// 查询节点元数据；不存在返回 [`FsError::NotFound`]。
    fn metadata(&self, rel_path: &str) -> FsResult<FsMetadata>;
    /// 读取普通文件内容；目录路径返回 [`FsError::NotAFile`]。
    fn read(&self, rel_path: &str) -> FsResult<Vec<u8>>;
    /// 读取普通文件指定区段；默认实现基于 [`Self::read`]，实现方可覆盖以避免整文件分配。
    fn read_range(&self, rel_path: &str, offset: u64, buf: &mut [u8]) -> FsResult<usize> {
        let data = self.read(rel_path)?;
        let start = offset as usize;
        if start >= data.len() {
            return Ok(0);
        }
        let n = core::cmp::min(buf.len(), data.len() - start);
        buf[..n].copy_from_slice(&data[start..start + n]);
        Ok(n)
    }
    /// 读取符号链接目标；非符号链接返回 [`FsError::NotAFile`]。
    fn read_symlink(&self, rel_path: &str) -> FsResult<Vec<u8>>;
    /// 列出目录项；非目录返回 [`FsError::NotAFile`]。
    fn read_dir(&self, rel_path: &str) -> FsResult<Vec<FsDirEntry>>;
}

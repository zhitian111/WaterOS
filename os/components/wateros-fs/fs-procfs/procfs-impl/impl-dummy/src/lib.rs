#![no_std]

//! procfs 空实现：所有路径不存在，用于无 task/VFS 回调的最小链接配置。
extern crate alloc;

use alloc::vec::Vec;
use api_v0::{FsError, FsMetadata, FsResult, ProcFsView};

/// 无状态的 [`api_v0::ProcFsView`]；不生成任何 `/proc` 内容。
pub struct DummyProcFs;

/// 返回全局唯一的 dummy 视图句柄。
pub fn view() -> &'static DummyProcFs { &DummyProcFs }

impl ProcFsView for DummyProcFs {
    fn exists(&self, _rel_path : &str) -> FsResult<bool> { Ok(false) }

    fn metadata(&self, _rel_path : &str) -> FsResult<FsMetadata> { Err(FsError::NotFound) }

    fn read(&self, _rel_path : &str) -> FsResult<Vec<u8>> { Err(FsError::NotFound) }

    fn read_symlink(&self, _rel_path : &str) -> FsResult<Vec<u8>> { Err(FsError::NotFound) }

    fn read_dir(&self, _rel_path : &str) -> FsResult<Vec<api_v0::FsDirEntry>> { Ok(Vec::new()) }
}

/// 占位：dummy 不消费 task 回调。
pub fn register_task_argv_lookup(_f : api_v0::TaskArgvLookup) {}

/// 占位：dummy 不消费 exe 回调。
pub fn register_task_exe_lookup(_f : api_v0::TaskExeLookup) {}

/// 占位：dummy 不消费挂载表回调。
pub fn register_mount_list_lookup(_f : api_v0::MountListLookup) {}

/// 占位：dummy 不消费 uptime 回调。
pub fn register_uptime_lookup(_f : api_v0::UptimeLookup) {}

/// 占位：dummy 不消费 idle time 回调。
pub fn register_idle_time_lookup(_f : api_v0::IdleTimeLookup) {}

/// 空自检。
pub fn test() {}

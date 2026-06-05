#![no_std]

extern crate alloc;

use alloc::vec::Vec;
use api_v0::{FsError, FsMetadata, FsNodeType, FsResult, ProcFsView};

pub struct DummyProcFs;

pub fn view() -> &'static DummyProcFs {
    &DummyProcFs
}

impl ProcFsView for DummyProcFs {
    fn exists(&self, _rel_path: &str) -> FsResult<bool> {
        Ok(false)
    }

    fn metadata(&self, _rel_path: &str) -> FsResult<FsMetadata> {
        Err(FsError::NotFound)
    }

    fn read(&self, _rel_path: &str) -> FsResult<Vec<u8>> {
        Err(FsError::NotFound)
    }

    fn read_dir(&self, _rel_path: &str) -> FsResult<Vec<api_v0::FsDirEntry>> {
        Ok(Vec::new())
    }
}

pub fn register_task_argv_lookup(_f: api_v0::TaskArgvLookup) {}
pub fn register_task_exe_lookup(_f: api_v0::TaskExeLookup) {}
pub fn register_mount_list_lookup(_f: api_v0::MountListLookup) {}

pub fn test() {}

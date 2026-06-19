//! Syscall-layer atime/mtime overrides until VFS metadata grows timestamp fields.

extern crate alloc;

use alloc::collections::BTreeMap;

use spin::Mutex;
use vfs::api::VfsMetadata;

use crate::linux_stat::{LinuxStat, LinuxStatx};

#[derive(Clone, Copy)]
pub(crate) struct StatTime {
    pub sec: i64,
    pub nsec: i64,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct FileKey {
    dev_major: u32,
    dev_minor: u32,
    inode: u64,
}

#[derive(Clone, Copy)]
struct FileTimes {
    atime: StatTime,
    mtime: StatTime,
}

static TIMES: Mutex<BTreeMap<FileKey, FileTimes>> = Mutex::new(BTreeMap::new());

fn key(meta: &VfsMetadata) -> FileKey {
    FileKey {
        dev_major: meta.device_major,
        dev_minor: meta.device_minor,
        inode: meta.inode,
    }
}

pub(crate) fn set(meta: &VfsMetadata, atime: Option<StatTime>, mtime: Option<StatTime>) {
    let mut times = TIMES.lock();
    let entry = times.entry(key(meta)).or_insert(FileTimes {
        atime: StatTime { sec: 0, nsec: 0 },
        mtime: StatTime { sec: 0, nsec: 0 },
    });
    if let Some(atime) = atime {
        entry.atime = atime;
    }
    if let Some(mtime) = mtime {
        entry.mtime = mtime;
    }
}

pub(crate) fn apply_stat(meta: &VfsMetadata, stat: &mut LinuxStat) {
    let Some(times) = TIMES.lock().get(&key(meta)).copied() else {
        return;
    };
    stat.st_atime_sec = times.atime.sec;
    stat.st_atime_nsec = times.atime.nsec;
    stat.st_mtime_sec = times.mtime.sec;
    stat.st_mtime_nsec = times.mtime.nsec;
}

pub(crate) fn apply_statx(meta: &VfsMetadata, statx: &mut LinuxStatx) {
    let Some(times) = TIMES.lock().get(&key(meta)).copied() else {
        return;
    };
    statx.stx_atime.tv_sec = times.atime.sec;
    statx.stx_atime.tv_nsec = times.atime.nsec as u32;
    statx.stx_mtime.tv_sec = times.mtime.sec;
    statx.stx_mtime.tv_nsec = times.mtime.nsec as u32;
}

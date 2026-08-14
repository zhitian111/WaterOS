#![no_std]

//! WaterOS adapter for the vendored `another_ext4` implementation.
//!
//! The upstream crate works with fixed 4096-byte filesystem blocks and a
//! synchronous block-device trait.  This module keeps that detail behind the
//! stable WaterOS filesystem API.

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use another_ext4::{Ext4, FileType, InodeMode, EXT4_ROOT_INO};
#[cfg(feature = "self_test")]
use another_ext4::BLOCK_SIZE;
use api_v0::{
    FsAccessMode, FsCapability, FsDirEntry, FsError, FsImpl, FsKind, FsMetadata, FsNodeId,
    FsNodeType, FsResult, LocalFs, LocalRwFs, ReadOnlyFs, ReadWriteFs, SharedFs, SharedRwFs,
};
use core::sync::atomic::AtomicBool;
#[cfg(any(feature = "lookup-diagnostics", test))]
use core::sync::atomic::Ordering;
#[cfg(feature = "lookup-diagnostics")]
use core::sync::atomic::AtomicU64;
use driver_block_api_v0::SharedBlockDevice;
use spin::Mutex;

const EXT4_SUPER_MAGIC : u16 = 0xEF53;
const SUPERBLOCK_MAGIC_OFFSET : u64 = 1024 + 0x38;
const LOOKUP_CACHE_CAPACITY : usize = 4096;
const OPEN_INODE_DIR : &str = "/.wateros-open-inodes";

#[path = "dentry_cache.rs"]
mod dentry_cache;
#[path = "block_io.rs"]
mod block_io;
#[path = "path_lookup.rs"]
mod path_lookup;
use block_io::{check_backend_error, map_error, map_type, BlockAdapter};
pub(crate) use path_lookup::{lookup, metadata, metadata_open, parent_name, write_with_ordered_size};
pub(crate) use dentry_cache::NegativeDentryCache;
#[cfg(test)]
pub(crate) use dentry_cache::negative_path_hash;

#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[fs/another-ext4] self_test begin");
    assert_eq!(BLOCK_SIZE, 4096);
    assert_eq!(EXT4_SUPER_MAGIC, 0xEF53);
    assert!(LOOKUP_CACHE_CAPACITY > 0);
    log::info!("[fs/another-ext4] self_test complete");
}

#[cfg(feature = "lookup-diagnostics")]
struct LookupDiagnostics {
    total : AtomicU64,
    positive_hit : AtomicU64,
    lookup_success : AtomicU64,
    not_found : AtomicU64,
    negative_hit : AtomicU64,
    positive_clear : AtomicU64,
    negative_invalidate : AtomicU64,
}

#[cfg(feature = "lookup-diagnostics")]
impl LookupDiagnostics {
    const fn new() -> Self {
        Self { total : AtomicU64::new(0),
               positive_hit : AtomicU64::new(0),
               lookup_success : AtomicU64::new(0),
               not_found : AtomicU64::new(0),
               negative_hit : AtomicU64::new(0),
               positive_clear : AtomicU64::new(0),
               negative_invalidate : AtomicU64::new(0) }
    }

    fn event(&self, counter : &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
        let total = self.total.fetch_add(1, Ordering::Relaxed) + 1;
        if total % (1 << 18) == 0 {
            log::info!("BUILDSTORM_FS_META_COUNTERS total={} positive_hit={} lookup_success={} not_found={} negative_hit={} positive_clear={} negative_invalidate={}",
                       total,
                       self.positive_hit.load(Ordering::Relaxed),
                       self.lookup_success.load(Ordering::Relaxed),
                       self.not_found.load(Ordering::Relaxed),
                       self.negative_hit.load(Ordering::Relaxed),
                       self.positive_clear.load(Ordering::Relaxed),
                       self.negative_invalidate.load(Ordering::Relaxed));
        }
    }
}

#[cfg(feature = "lookup-diagnostics")]
static LOOKUP_DIAGNOSTICS : LookupDiagnostics = LookupDiagnostics::new();

macro_rules! lookup_diag_event {
    ($counter:ident) => {
        #[cfg(feature = "lookup-diagnostics")]
        LOOKUP_DIAGNOSTICS.event(&LOOKUP_DIAGNOSTICS.$counter)
    };
}

fn lookup_diag_positive_clear() {
    #[cfg(feature = "lookup-diagnostics")]
    LOOKUP_DIAGNOSTICS.positive_clear.fetch_add(1, Ordering::Relaxed);
}

fn lookup_diag_negative_invalidate(removed : usize) {
    #[cfg(feature = "lookup-diagnostics")]
    LOOKUP_DIAGNOSTICS.negative_invalidate.fetch_add(removed as u64, Ordering::Relaxed);
    #[cfg(not(feature = "lookup-diagnostics"))]
    let _ = removed;
}



#[path = "filesystem.rs"]
mod filesystem;
pub(crate) use filesystem::*;
#[path = "backend.rs"]
mod backend;
pub use backend::*;

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
use another_ext4::{
    Block, BlockDevice, ErrCode, Ext4, Ext4Error, FileType, InodeMode, BLOCK_SIZE, EXT4_ROOT_INO,
};
use api_v0::{
    FsAccessMode, FsCapability, FsDirEntry, FsError, FsImpl, FsKind, FsMetadata, FsNodeId,
    FsNodeType, FsResult, LocalFs, LocalRwFs, ReadOnlyFs, ReadWriteFs, SharedFs, SharedRwFs,
};
use core::sync::atomic::{AtomicBool, Ordering};
#[cfg(feature = "lookup-diagnostics")]
use core::sync::atomic::AtomicU64;
use driver_block_api_v0::{Lba, SharedBlockDevice};
use spin::Mutex;

const EXT4_SUPER_MAGIC : u16 = 0xEF53;
const SUPERBLOCK_MAGIC_OFFSET : u64 = 1024 + 0x38;
const LOOKUP_CACHE_CAPACITY : usize = 4096;
const NEGATIVE_CACHE_CAPACITY : usize = 4096;
const NEGATIVE_CACHE_WAYS : usize = 4;
const NEGATIVE_CACHE_BUCKETS : usize = NEGATIVE_CACHE_CAPACITY / NEGATIVE_CACHE_WAYS;
const OPEN_INODE_DIR : &str = "/.wateros-open-inodes";

#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[fs/another-ext4] self_test begin");
    assert_eq!(BLOCK_SIZE, 4096);
    assert_eq!(EXT4_SUPER_MAGIC, 0xEF53);
    assert!(LOOKUP_CACHE_CAPACITY > 0);
    log::info!("[fs/another-ext4] self_test complete");
}

fn map_error(error : Ext4Error) -> FsError {
    match error.code() {
        ErrCode::ENOENT => FsError::NotFound,
        ErrCode::EEXIST => FsError::Exists,
        ErrCode::ENOTEMPTY => FsError::NotEmpty,
        ErrCode::ENOTDIR | ErrCode::EISDIR => FsError::NotAFile,
        ErrCode::EINVAL => FsError::InvalidPath,
        ErrCode::ENOSPC => FsError::NoSpace,
        ErrCode::EROFS | ErrCode::ENOTSUP => FsError::Unsupported,
        ErrCode::EIO => FsError::Io,
        _ => FsError::Io,
    }
}

fn map_type(file_type : FileType) -> FsNodeType {
    match file_type {
        FileType::RegularFile => FsNodeType::File,
        FileType::Directory => FsNodeType::Directory,
        FileType::SymLink => FsNodeType::Symlink,
        _ => FsNodeType::Special,
    }
}

/// Adapts WaterOS's 512-byte-LBA block device to another_ext4's 4096-byte blocks.
struct BlockAdapter {
    device : SharedBlockDevice,
    io_error : Arc<AtomicBool>,
}

impl BlockDevice for BlockAdapter {
    fn read_block(&self, block_id : u64) -> Block {
        let mut data = Box::new([0u8; BLOCK_SIZE]);
        let mut guard = self.device.lock();
        let block_size = guard.block_size() as u64;
        if block_size == 0 || BLOCK_SIZE as u64 % block_size != 0 {
            self.io_error.store(true, Ordering::Release);
            log::error!(
                "[fs::another-ext4] unsupported device block size {block_size}, block={block_id}"
            );
            return Block::new(block_id, data);
        }
        guard.read_blocks(Lba(block_id * (BLOCK_SIZE as u64 / block_size)),
                          &mut data[..])
             .unwrap_or_else(|error| {
                 self.io_error.store(true, Ordering::Release);
                 log::error!("[fs::another-ext4] failed to read block {block_id}: {error:?}");
             });
        Block::new(block_id, data)
    }

    fn write_block(&self, block : &Block) {
        let mut guard = self.device.lock();
        let block_size = guard.block_size();
        if block_size == 0 || BLOCK_SIZE % block_size != 0 {
            self.io_error.store(true, Ordering::Release);
            log::error!(
                "[fs::another-ext4] unsupported device block size {block_size}, block={}", block.id
            );
            return;
        }
        let lba_count = BLOCK_SIZE / block_size;
        guard.write_blocks(Lba(block.id * lba_count as u64), &block.data[..])
             .unwrap_or_else(|error| {
                 self.io_error.store(true, Ordering::Release);
                 log::error!("[fs::another-ext4] failed to write block {}: {error:?}", block.id);
             });
    }
}

fn probe(device : &SharedBlockDevice) -> FsResult<bool> {
    let mut bytes = [0u8; 2];
    device.lock()
          .read_bytes(SUPERBLOCK_MAGIC_OFFSET, &mut bytes)
          .map_err(|_| FsError::Driver)?;
    Ok(u16::from_le_bytes(bytes) == EXT4_SUPER_MAGIC)
}

fn lookup(fs : &Ext4, path : &str) -> FsResult<u32> {
    if path == "/" || path.is_empty() {
        return Ok(EXT4_ROOT_INO);
    }
    if !path.starts_with('/') ||
       path.split('/')
           .any(|part| part == "." || part == "..")
    {
        return Err(FsError::InvalidPath);
    }
    fs.generic_lookup(EXT4_ROOT_INO, path)
      .map_err(map_error)
}

fn metadata(fs : &Ext4, inode : u32) -> FsResult<FsMetadata> {
    let attr = fs.getattr(inode)
                 .map_err(map_error)?;
    let mode = InodeMode::from_type_and_perm(attr.ftype, attr.perm).bits();
    Ok(FsMetadata { node_type : map_type(attr.ftype),
                    size : attr.size,
                    mode,
                    inode : attr.ino as u64,
                    nlink : attr.links as u32,
                    uid : attr.uid,
                    gid : attr.gid })
}

fn write_with_ordered_size(fs : &Ext4,
                           inode : u32,
                           offset : u64,
                           data : &[u8])
                           -> FsResult<usize> {
    let data_len = u64::try_from(data.len()).map_err(|_| FsError::NoSpace)?;
    let end = offset.checked_add(data_len).ok_or(FsError::NoSpace)?;
    let offset = usize::try_from(offset).map_err(|_| FsError::NoSpace)?;

    // another_ext4 normally allocates extents before updating i_size.  Commit an
    // extending size first so a reset can leave a sparse file, never extents past EOF.
    if end > fs.getattr(inode).map_err(map_error)?.size {
        fs.setattr(inode, None, None, None, Some(end), None, None, None, None)
          .map_err(map_error)?;
        fs.flush_all();
    }
    fs.write(inode, offset, data).map_err(map_error)?;
    fs.flush_all();
    Ok(data.len())
}

fn parent_name(path : &str) -> FsResult<(&str, &str)> {
    let path = path.trim_end_matches('/');
    let (parent, name) = path.rsplit_once('/').ok_or(FsError::InvalidPath)?;
    if name.is_empty() || name.len() > 255 || name == "." || name == ".." {
        return Err(FsError::InvalidPath);
    }
    Ok((if parent.is_empty() { "/" } else { parent }, name))
}

const FNV1A_OFFSET : u64 = 0xcbf2_9ce4_8422_2325;
const FNV1A_PRIME : u64 = 0x0000_0100_0000_01b3;

fn negative_path_hash(path : &str) -> u64 {
    path.as_bytes()
        .iter()
        .fold(FNV1A_OFFSET, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(FNV1A_PRIME)
        })
}

struct NegativeDentry {
    hash : u64,
    path : String,
}

struct NegativeDentryCache {
    slots : Vec<Option<NegativeDentry>>,
    next_victim : Vec<u8>,
}

impl NegativeDentryCache {
    fn new() -> Self {
        let mut slots = Vec::with_capacity(NEGATIVE_CACHE_CAPACITY);
        slots.resize_with(NEGATIVE_CACHE_CAPACITY, || None);
        Self { slots,
               next_victim : vec![0; NEGATIVE_CACHE_BUCKETS] }
    }

    fn bucket(hash : u64) -> usize { hash as usize % NEGATIVE_CACHE_BUCKETS }

    fn contains(&self, path : &str) -> bool {
        let hash = negative_path_hash(path);
        let first = Self::bucket(hash) * NEGATIVE_CACHE_WAYS;
        self.slots[first..first + NEGATIVE_CACHE_WAYS]
            .iter()
            .flatten()
            .any(|entry| entry.hash == hash && entry.path == path)
    }

    fn insert(&mut self, path : &str) {
        let hash = negative_path_hash(path);
        let bucket = Self::bucket(hash);
        let first = bucket * NEGATIVE_CACHE_WAYS;
        let ways = &mut self.slots[first..first + NEGATIVE_CACHE_WAYS];
        if ways.iter()
               .flatten()
               .any(|entry| entry.hash == hash && entry.path == path)
        {
            return;
        }
        let way = ways.iter()
                      .position(Option::is_none)
                      .unwrap_or_else(|| {
                          let way = usize::from(self.next_victim[bucket]);
                          self.next_victim[bucket] =
                              ((way + 1) % NEGATIVE_CACHE_WAYS) as u8;
                          way
                      });
        ways[way] = Some(NegativeDentry { hash,
                                          path : String::from(path) });
    }

    fn remove_exact(&mut self, path : &str) -> usize {
        let hash = negative_path_hash(path);
        let first = Self::bucket(hash) * NEGATIVE_CACHE_WAYS;
        for slot in self.slots[first..first + NEGATIVE_CACHE_WAYS].iter_mut() {
            if slot.as_ref()
                   .is_some_and(|entry| entry.hash == hash && entry.path == path)
            {
                *slot = None;
                return 1;
            }
        }
        0
    }

    fn remove_subtree(&mut self, path : &str) -> usize {
        let prefix = if path.ends_with('/') {
            String::from(path)
        } else {
            let mut prefix = String::from(path);
            prefix.push('/');
            prefix
        };
        let mut removed = 0usize;
        for slot in self.slots.iter_mut() {
            let matches = slot.as_ref()
                              .is_some_and(|entry| {
                                  entry.path == path || entry.path.starts_with(prefix.as_str())
                              });
            if matches {
                *slot = None;
                removed += 1;
            }
        }
        removed
    }
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

pub struct AnotherExt4Fs {
    fs : Option<Ext4>,
    io_error_state : Option<Arc<AtomicBool>>,
    lookup_cache : Mutex<BTreeMap<String, u32>>,
    negative_cache : Mutex<Option<Box<NegativeDentryCache>>>,
    open_nodes : BTreeMap<u32, usize>,
    orphan_nodes : BTreeMap<u32, String>,
    orphan_dir : Option<u32>,
}

impl AnotherExt4Fs {
    const fn new() -> Self {
        Self { fs : None,
               io_error_state : None,
               lookup_cache : Mutex::new(BTreeMap::new()),
               negative_cache : Mutex::new(None),
               open_nodes : BTreeMap::new(),
               orphan_nodes : BTreeMap::new(),
               orphan_dir : None }
    }
    fn get(&self) -> FsResult<&Ext4> {
        self.check_backend()?;
        self.fs
            .as_ref()
            .ok_or(FsError::NotMounted)
    }

    fn get_mut(&mut self) -> FsResult<&mut Ext4> {
        self.check_backend()?;
        self.fs.as_mut().ok_or(FsError::NotMounted)
    }

    fn check_backend(&self) -> FsResult<()> {
        check_backend_error(&self.io_error_state)
    }

    fn lookup(&self, path : &str) -> FsResult<u32> {
        if let Some(inode) = self.lookup_cache.lock().get(path).copied() {
            lookup_diag_event!(positive_hit);
            return Ok(inode);
        }
        if self.negative_cache
               .lock()
               .as_ref()
               .is_some_and(|cache| cache.contains(path))
        {
            lookup_diag_event!(negative_hit);
            return Err(FsError::NotFound);
        }
        match lookup(self.get()?, path) {
            Ok(inode) => {
                lookup_diag_event!(lookup_success);
                self.cache_insert(path, inode);
                Ok(inode)
            }
            Err(FsError::NotFound) => {
                lookup_diag_event!(not_found);
                self.negative_cache
                    .lock()
                    .get_or_insert_with(|| Box::new(NegativeDentryCache::new()))
                    .insert(path);
                Err(FsError::NotFound)
            }
            Err(error) => Err(error),
        }
    }

    fn cache_insert(&self, path : &str, inode : u32) {
        let mut cache = self.lookup_cache.lock();
        if cache.len() >= LOOKUP_CACHE_CAPACITY && !cache.contains_key(path) {
            cache.clear();
            lookup_diag_positive_clear();
        }
        cache.insert(String::from(path), inode);
        drop(cache);
        self.negative_cache_remove_exact(path);
    }

    fn negative_cache_remove_exact(&self, path : &str) {
        let removed = self.negative_cache
                          .lock()
                          .as_mut()
                          .map(|cache| cache.remove_exact(path))
                          .unwrap_or(0);
        lookup_diag_negative_invalidate(removed);
    }

    fn negative_cache_remove_subtree(&self, path : &str) {
        let removed = self.negative_cache
                          .lock()
                          .as_mut()
                          .map(|cache| cache.remove_subtree(path))
                          .unwrap_or(0);
        lookup_diag_negative_invalidate(removed);
    }

    fn cache_remove_subtree(&self, path : &str) {
        let prefix = if path.ends_with('/') {
            String::from(path)
        } else {
            let mut prefix = String::from(path);
            prefix.push('/');
            prefix
        };
        self.lookup_cache
            .lock()
            .retain(|cached, _| cached != path && !cached.starts_with(prefix.as_str()));
    }

    fn cache_rename_subtree(&self, old_path : &str, new_path : &str) {
        let old_prefix = if old_path.ends_with('/') {
            String::from(old_path)
        } else {
            let mut prefix = String::from(old_path);
            prefix.push('/');
            prefix
        };
        let new_prefix = if new_path.ends_with('/') {
            String::from(new_path)
        } else {
            let mut prefix = String::from(new_path);
            prefix.push('/');
            prefix
        };
        let mut moved = Vec::new();
        let mut cache = self.lookup_cache.lock();
        cache.retain(|cached, inode| {
            if cached == old_path {
                moved.push((String::from(new_path), *inode));
                return false;
            }
            if let Some(suffix) = cached.strip_prefix(old_prefix.as_str()) {
                let mut renamed = String::from(new_path.trim_end_matches('/'));
                renamed.push('/');
                renamed.push_str(suffix);
                moved.push((renamed, *inode));
                return false;
            }
            cached != new_path && !cached.starts_with(new_prefix.as_str())
        });
        for (path, inode) in moved {
            cache.insert(path, inode);
        }
        drop(cache);
        self.negative_cache_remove_subtree(new_path);
    }

    fn open_inode(&self, node : FsNodeId) -> FsResult<u32> {
        let inode = u32::try_from(node.raw()).map_err(|_| FsError::InvalidPath)?;
        self.open_nodes
            .contains_key(&inode)
            .then_some(inode)
            .ok_or(FsError::NotFound)
    }

    fn cleanup_stale_orphans(&mut self) -> FsResult<()> {
        let fs = self.get_mut()?;
        let dir = match lookup(fs, OPEN_INODE_DIR) {
            Ok(dir) => dir,
            Err(FsError::NotFound) => return Ok(()),
            Err(error) => return Err(error),
        };
        if metadata(fs, dir)?.node_type != FsNodeType::Directory {
            return Err(FsError::InvalidPath);
        }
        let names : Vec<String> = fs.listdir(dir)
                                    .map_err(map_error)?
                                    .into_iter()
                                    .filter(|entry| {
                                        let name = entry.name();
                                        name != "." && name != ".." && !entry.unused()
                                    })
                                    .map(|entry| entry.name())
                                    .collect();
        let had_stale = !names.is_empty();
        for name in names {
            fs.unlink(dir, name.as_str()).map_err(map_error)?;
        }
        if had_stale {
            fs.flush_all();
        }
        self.orphan_dir = Some(dir);
        Ok(())
    }

    fn ensure_orphan_dir(&mut self) -> FsResult<u32> {
        if let Some(dir) = self.orphan_dir {
            return Ok(dir);
        }
        let fs = self.get_mut()?;
        let (dir, created) = match lookup(fs, OPEN_INODE_DIR) {
            Ok(dir) => (dir, false),
            Err(FsError::NotFound) => {
                (fs.mkdir(EXT4_ROOT_INO,
                          OPEN_INODE_DIR.trim_start_matches('/'),
                          InodeMode::DIRECTORY | InodeMode::from_bits_retain(0o700))
                   .map_err(map_error)?,
                 true)
            }
            Err(error) => return Err(error),
        };
        if metadata(fs, dir)?.node_type != FsNodeType::Directory {
            return Err(FsError::InvalidPath);
        }
        self.orphan_dir = Some(dir);
        if created {
            self.cache_insert(OPEN_INODE_DIR, dir);
        }
        Ok(dir)
    }

    fn preserve_inode_if_open(&mut self, inode : u32) -> FsResult<()> {
        if !self.open_nodes.contains_key(&inode) || self.orphan_nodes.contains_key(&inode) {
            return Ok(());
        }
        let dir = self.ensure_orphan_dir()?;
        let name = alloc::format!("{inode:08x}");
        self.get_mut()?.link(inode, dir, name.as_str()).map_err(map_error)?;
        self.get_mut()?.flush_all();
        let mut path = String::from(OPEN_INODE_DIR);
        path.push('/');
        path.push_str(name.as_str());
        self.orphan_nodes.insert(inode, name);
        self.cache_insert(path.as_str(), inode);
        Ok(())
    }
}

impl ReadOnlyFs for AnotherExt4Fs {
    fn mount(&mut self, device : SharedBlockDevice) -> FsResult<()> {
        let io_error_state = Arc::new(AtomicBool::new(false));
        let backend = Arc::new(BlockAdapter { device, io_error : io_error_state.clone() });
        let fs = Ext4::load(backend).map_err(|error| {
            log::error!(
                "[fs::another-ext4] mount failed: code={:?} detail={:?}",
                error.code(),
                error
            );
            map_error(error)
        })?;
        let state = Some(io_error_state);
        check_backend_error(&state)?;
        self.io_error_state = state;
        self.fs = Some(fs);
        self.lookup_cache.lock().clear();
        self.negative_cache.lock().take();
        self.open_nodes.clear();
        self.orphan_nodes.clear();
        self.orphan_dir = None;
        Ok(())
    }

    fn is_mounted(&self) -> bool { self.fs.is_some() }

    fn exists(&self, path : &str) -> FsResult<bool> {
        let result = match self.lookup(path) {
            Ok(_) => Ok(true),
            Err(FsError::NotFound) => Ok(false),
            Err(error) => Err(error),
        };
        self.check_backend()?;
        result
    }

    fn metadata(&self, path : &str) -> FsResult<FsMetadata> {
        let result = metadata(self.get()?, self.lookup(path)?);
        self.check_backend()?;
        result
    }

    fn read_range(&self, path : &str, offset : u64, buf : &mut [u8]) -> FsResult<usize> {
        let fs = self.get()?;
        let inode = self.lookup(path)?;
        let result = fs.read(inode, offset as usize, buf).map_err(|error| {
            log::error!("[fs::another-ext4] read failed path={} inode={} offset={} len={} code={:?}",
                        path,
                        inode,
                        offset,
                        buf.len(),
                        error.code());
            map_error(error)
        });
        self.check_backend()?;
        result
    }

    fn read(&self, path : &str) -> FsResult<Vec<u8>> {
        let attr = ReadOnlyFs::metadata(self, path)?;
        if attr.node_type != FsNodeType::File {
            return Err(FsError::NotAFile);
        }
        let mut data = vec![0; attr.size as usize];
        let len = ReadOnlyFs::read_range(self, path, 0, &mut data)?;
        data.truncate(len);
        Ok(data)
    }

    fn read_dir(&self, path : &str) -> FsResult<Vec<FsDirEntry>> {
        let fs = self.get()?;
        let inode = self.lookup(path)?;
        let attr = fs.getattr(inode)
                     .map_err(map_error)?;
        if map_type(attr.ftype) != FsNodeType::Directory {
            return Err(FsError::NotAFile);
        }
        let mut entries = Vec::new();
        for entry in fs.listdir(inode)
                       .map_err(map_error)?
        {
            let name = entry.name();
            if name == "." || name == ".." || entry.unused() {
                continue;
            }
            let child = fs.getattr(entry.inode())
                          .map_err(map_error)?;
            entries.push(FsDirEntry { name,
                                      node_type : map_type(child.ftype) });
        }
        self.check_backend()?;
        Ok(entries)
    }

    fn read_symlink(&self, path : &str) -> FsResult<Vec<u8>> {
        let fs = self.get()?;
        let inode = self.lookup(path)?;
        let attr = fs.getattr(inode)
                     .map_err(map_error)?;
        if map_type(attr.ftype) != FsNodeType::Symlink {
            return Err(FsError::NotAFile);
        }
        let mut data = vec![0; attr.size as usize];
        let len = fs.readlink(inode, 0, &mut data)
                    .map_err(map_error)?;
        self.check_backend()?;
        data.truncate(len);
        Ok(data)
    }
}

fn check_backend_error(io_error_state : &Option<Arc<AtomicBool>>) -> FsResult<()> {
    if io_error_state.as_ref().is_some_and(|state| state.load(Ordering::Acquire)) {
        return Err(FsError::Io);
    }
    Ok(())
}

impl ReadWriteFs for AnotherExt4Fs {
    fn mount_rw(&mut self, device : SharedBlockDevice) -> FsResult<()> {
        self.mount(device)?;
        let result = self.cleanup_stale_orphans();
        self.check_backend()?;
        result
    }
    fn is_mounted(&self) -> bool { self.fs.is_some() }

    fn sync(&mut self) -> FsResult<()> {
        self.get_mut()?.flush_all();
        self.check_backend()?;
        Ok(())
    }

    fn open_node(&mut self, path : &str) -> FsResult<FsNodeId> {
        let inode = self.lookup(path)?;
        if metadata(self.get()?, inode)?.node_type != FsNodeType::File {
            return Err(FsError::NotAFile);
        }
        let count = self.open_nodes.entry(inode).or_insert(0);
        *count = count.checked_add(1).ok_or(FsError::NoSpace)?;
        self.check_backend()?;
        Ok(FsNodeId::new(inode as u64))
    }

    fn close_node(&mut self, node : FsNodeId) -> FsResult<()> {
        let inode = self.open_inode(node)?;
        let count = *self.open_nodes.get(&inode).ok_or(FsError::NotFound)?;
        if count > 1 {
            self.open_nodes.insert(inode, count - 1);
            return Ok(());
        }
        if count == 0 {
            return Err(FsError::Io);
        }
        if let Some(name) = self.orphan_nodes.get(&inode).cloned() {
            let dir = self.orphan_dir.ok_or(FsError::Io)?;
            self.get_mut()?.unlink(dir, name.as_str()).map_err(map_error)?;
            self.get_mut()?.flush_all();
            self.check_backend()?;
            self.orphan_nodes.remove(&inode);
        }
        self.open_nodes.remove(&inode);
        Ok(())
    }

    fn metadata_node(&self, node : FsNodeId) -> FsResult<FsMetadata> {
        let result = metadata(self.get()?, self.open_inode(node)?);
        self.check_backend()?;
        result
    }

    fn read_range_node(&self,
                       node : FsNodeId,
                       offset : u64,
                       buf : &mut [u8])
                       -> FsResult<usize> {
        let result = self.get()?.read(self.open_inode(node)?, offset as usize, buf).map_err(map_error);
        self.check_backend()?;
        result
    }

    fn write_range_node(&mut self,
                        node : FsNodeId,
                        offset : u64,
                        data : &[u8])
                        -> FsResult<usize> {
        let inode = self.open_inode(node)?;
        let result = write_with_ordered_size(self.get_mut()?, inode, offset, data);
        self.check_backend()?;
        result
    }

    fn truncate_node(&mut self, node : FsNodeId, len : u64) -> FsResult<()> {
        let inode = self.open_inode(node)?;
        self.get_mut()?.setattr(inode, None, None, None, Some(len), None, None, None, None)
                       .map_err(map_error)?;
        self.get_mut()?.flush_all();
        self.check_backend()?;
        Ok(())
    }

    fn exists(&self, path : &str) -> FsResult<bool> { ReadOnlyFs::exists(self, path) }

    fn metadata(&self, path : &str) -> FsResult<FsMetadata> {
        ReadOnlyFs::metadata(self, path)
    }

    fn read(&self, path : &str) -> FsResult<Vec<u8>> { ReadOnlyFs::read(self, path) }

    fn read_range(&self, path : &str, offset : u64, buf : &mut [u8]) -> FsResult<usize> {
        ReadOnlyFs::read_range(self, path, offset, buf)
    }

    fn read_dir(&self, path : &str) -> FsResult<Vec<FsDirEntry>> {
        ReadOnlyFs::read_dir(self, path)
    }

    fn read_symlink(&self, path : &str) -> FsResult<Vec<u8>> {
        ReadOnlyFs::read_symlink(self, path)
    }

    fn write_regular_file_at_root(&mut self, name : &str, data : &[u8]) -> FsResult<()> {
        let mut path = String::from("/");
        path.push_str(name);
        self.write_regular_file(&path, data)
    }

    fn write_regular_file(&mut self, path : &str, data : &[u8]) -> FsResult<()> {
        let fs = self.get_mut()?;
        let (inode, created) = match lookup(fs, path) {
            Ok(inode) => (inode, false),
            Err(FsError::NotFound) => (fs.generic_create(EXT4_ROOT_INO,
                                                         path,
                                                         InodeMode::FILE | InodeMode::ALL_RW)
                                         .map_err(map_error)?, true),
            Err(error) => return Err(error),
        };
        fs.setattr(inode, None, None, None, Some(0), None, None, None, None)
          .map_err(map_error)?;
        write_with_ordered_size(fs, inode, 0, data)?;
        fs.flush_all();
        self.check_backend()?;
        if created {
            self.cache_insert(path, inode);
        }
        Ok(())
    }

    fn unlink(&mut self, path : &str) -> FsResult<()> {
        let inode = self.lookup(path)?;
        if metadata(self.get()?, inode)?.node_type == FsNodeType::Directory {
            return Err(FsError::NotAFile);
        }
        self.preserve_inode_if_open(inode)?;
        self.get_mut()?.generic_remove(EXT4_ROOT_INO, path).map_err(map_error)?;
        self.get_mut()?.flush_all();
        self.check_backend()?;
        self.cache_remove_subtree(path);
        Ok(())
    }

    fn rmdir(&mut self, path : &str) -> FsResult<()> {
        let fs = self.get_mut()?;
        let inode = lookup(fs, path)?;
        if metadata(fs, inode)?.node_type != FsNodeType::Directory {
            return Err(FsError::NotAFile);
        }
        fs.generic_remove(EXT4_ROOT_INO, path).map_err(map_error)?;
        fs.flush_all();
        self.check_backend()?;
        self.cache_remove_subtree(path);
        Ok(())
    }

    fn write_range(&mut self, path : &str, offset : u64, data : &[u8]) -> FsResult<usize> {
        let fs = self.get_mut()?;
        let inode = lookup(fs, path)?;
        let result = write_with_ordered_size(fs, inode, offset, data);
        self.check_backend()?;
        result
    }

    fn truncate(&mut self, path : &str, len : u64) -> FsResult<()> {
        let fs = self.get_mut()?;
        let inode = lookup(fs, path)?;
        fs.setattr(inode, None, None, None, Some(len), None, None, None, None)
          .map_err(map_error)?;
        fs.flush_all();
        self.check_backend()?;
        Ok(())
    }

    fn mkdir(&mut self, path : &str, mode : u32) -> FsResult<()> {
        let fs = self.get_mut()?;
        match lookup(fs, path) {
            Ok(_) => return Err(FsError::Exists),
            Err(FsError::NotFound) => {}
            Err(error) => return Err(error),
        }
        let (parent, name) = parent_name(path)?;
        let parent = lookup(fs, parent)?;
        let inode = fs.mkdir(parent,
                             name,
                             InodeMode::DIRECTORY | InodeMode::from_bits_retain(mode as u16))
                      .map_err(map_error)?;
        fs.flush_all();
        self.check_backend()?;
        self.cache_insert(path, inode);
        Ok(())
    }

    fn chmod(&mut self, path : &str, mode : u32) -> FsResult<()> {
        let fs = self.get_mut()?;
        let inode = lookup(fs, path)?;
        let file_type = fs.getattr(inode).map_err(map_error)?.ftype;
        let mode = InodeMode::from_type_and_perm(file_type,
                                                 InodeMode::from_bits_retain(mode as u16));
        fs.setattr(inode, Some(mode), None, None, None, None, None, None, None)
          .map_err(map_error)?;
        fs.flush_all();
        self.check_backend()?;
        Ok(())
    }

    fn chown(&mut self, path : &str, uid : Option<u32>, gid : Option<u32>) -> FsResult<()> {
        let fs = self.get_mut()?;
        let inode = lookup(fs, path)?;
        fs.setattr(inode, None, uid, gid, None, None, None, None, None)
          .map_err(map_error)?;
        fs.flush_all();
        self.check_backend()?;
        Ok(())
    }

    fn mknod(&mut self, path : &str, mode : u32, rdev : u32) -> FsResult<()> {
        let fs = self.get_mut()?;
        let inode_mode = InodeMode::from_bits_retain(mode as u16);
        match inode_mode.file_type() {
            FileType::RegularFile | FileType::Fifo | FileType::Socket => {}
            FileType::CharacterDev | FileType::BlockDev if rdev == 0 => {}
            _ => return Err(FsError::Unsupported),
        }
        let inode = fs.generic_create(EXT4_ROOT_INO, path, inode_mode)
                      .map_err(map_error)?;
        fs.flush_all();
        self.check_backend()?;
        self.cache_insert(path, inode);
        Ok(())
    }

    fn rename(&mut self, old_path : &str, new_path : &str) -> FsResult<()> {
        let fs = self.get_mut()?;
        fs.generic_rename(EXT4_ROOT_INO, old_path, new_path).map_err(map_error)?;
        fs.flush_all();
        self.check_backend()?;
        self.cache_rename_subtree(old_path, new_path);
        Ok(())
    }

    fn hardlink(&mut self, existing_path : &str, new_path : &str) -> FsResult<()> {
        let fs = self.get_mut()?;
        let child = lookup(fs, existing_path)?;
        let child_meta = metadata(fs, child)?;
        if child_meta.node_type == FsNodeType::Directory {
            return Err(FsError::NotAFile);
        }
        if child_meta.node_type != FsNodeType::File {
            return Err(FsError::Unsupported);
        }

        let (parent_path, name) = parent_name(new_path)?;
        let parent = lookup(fs, parent_path)?;
        if metadata(fs, parent)?.node_type != FsNodeType::Directory {
            return Err(FsError::NotAFile);
        }
        if lookup(fs, new_path).is_ok() {
            return Err(FsError::Exists);
        }

        fs.link(child, parent, name).map_err(map_error)?;
        fs.flush_all();
        self.check_backend()?;
        self.cache_insert(new_path, child);
        Ok(())
    }
}

pub struct AnotherExt4Impl;
pub static IMPL : AnotherExt4Impl = AnotherExt4Impl;

const SUPPORTED : &[FsCapability] = &[FsCapability::new(FsKind::Ext4, FsAccessMode::ReadOnly),
                                      FsCapability::new(FsKind::Ext4, FsAccessMode::ReadWrite)];

impl FsImpl for AnotherExt4Impl {
    fn name(&self) -> &'static str { "another-ext4" }
    fn supported(&self) -> &'static [FsCapability] { SUPPORTED }
    fn probe(&self, device : &SharedBlockDevice) -> FsResult<Option<FsKind>> {
        Ok(probe(device)?.then_some(FsKind::Ext4))
    }
    fn mount_ro(&self, device : SharedBlockDevice) -> FsResult<SharedFs> {
        let mut fs = AnotherExt4Fs::new();
        ReadOnlyFs::mount(&mut fs, device)?;
        Ok(Arc::new(Mutex::new(LocalFs::new(Box::new(fs)))))
    }
    fn mount_rw(&self, device : SharedBlockDevice) -> FsResult<SharedRwFs> {
        let mut fs = AnotherExt4Fs::new();
        ReadWriteFs::mount_rw(&mut fs, device)?;
        Ok(Arc::new(Mutex::new(LocalRwFs::new(Box::new(fs)))))
    }
}

#[cfg(test)]
mod tests {
    use super::{AnotherExt4Fs, AtomicBool, FsError, FsNodeId, Ordering, ReadWriteFs,
                check_backend_error};
    use alloc::boxed::Box;
    use alloc::sync::Arc;

    #[test]
    fn backend_error_latch_reports_io_after_failure() {
        let state = Some(Arc::new(AtomicBool::new(false)));
        assert_eq!(check_backend_error(&state), Ok(()));
        state.as_ref().unwrap().store(true, Ordering::Release);
        assert_eq!(check_backend_error(&state), Err(FsError::Io));
    }

    #[test]
    fn lookup_cache_rename_moves_only_source_subtree() {
        let fs = AnotherExt4Fs::new();
        fs.cache_insert("/src", 10);
        fs.cache_insert("/src/child", 11);
        fs.cache_insert("/dst/stale", 12);
        fs.cache_insert("/unrelated", 13);
        {
            let mut negative = fs.negative_cache.lock();
            let negative = negative.get_or_insert_with(|| Box::new(super::NegativeDentryCache::new()));
            negative.insert("/dst");
            negative.insert("/dst/missing");
            negative.insert("/dstish/missing");
        }

        fs.cache_rename_subtree("/src", "/dst");

        let cache = fs.lookup_cache.lock();
        assert_eq!(cache.get("/dst"), Some(&10));
        assert_eq!(cache.get("/dst/child"), Some(&11));
        assert_eq!(cache.get("/unrelated"), Some(&13));
        assert!(!cache.contains_key("/src"));
        assert!(!cache.contains_key("/src/child"));
        assert!(!cache.contains_key("/dst/stale"));
        drop(cache);
        let negative = fs.negative_cache.lock();
        let negative = negative.as_ref().unwrap();
        assert!(!negative.contains("/dst"));
        assert!(!negative.contains("/dst/missing"));
        assert!(negative.contains("/dstish/missing"));
    }

    #[test]
    fn stable_node_refcount_closes_exactly_once() {
        let mut fs = AnotherExt4Fs::new();
        fs.open_nodes.insert(42, 2);
        let node = FsNodeId::new(42);

        fs.close_node(node).unwrap();
        assert_eq!(fs.open_nodes.get(&42), Some(&1));
        fs.close_node(node).unwrap();
        assert!(!fs.open_nodes.contains_key(&42));
        assert_eq!(fs.close_node(node), Err(FsError::NotFound));
    }

    #[test]
    fn lookup_cache_remove_invalidates_descendants_only() {
        let fs = AnotherExt4Fs::new();
        fs.cache_insert("/tmp/work", 20);
        fs.cache_insert("/tmp/work/output", 21);
        fs.cache_insert("/tmp/worker", 22);

        fs.cache_remove_subtree("/tmp/work");

        let cache = fs.lookup_cache.lock();
        assert!(!cache.contains_key("/tmp/work"));
        assert!(!cache.contains_key("/tmp/work/output"));
        assert_eq!(cache.get("/tmp/worker"), Some(&22));
    }

    #[test]
    fn negative_cache_requires_full_path_match_and_removes_exact_entry() {
        let mut cache = super::NegativeDentryCache::new();
        let original = "/missing/a";
        let bucket = super::NegativeDentryCache::bucket(super::negative_path_hash(original));
        let collision = (0..10_000)
                            .map(|index| alloc::format!("/collision/{index}"))
                            .find(|path| {
                                path != original &&
                                super::NegativeDentryCache::bucket(
                                    super::negative_path_hash(path.as_str()),
                                ) == bucket
                            })
                            .expect("find another path in the same cache bucket");
        cache.insert(original);
        assert!(cache.contains(original));
        assert!(!cache.contains(collision.as_str()));
        assert_eq!(cache.remove_exact(collision.as_str()), 0);
        assert_eq!(cache.remove_exact(original), 1);
        assert!(!cache.contains(original));
    }

    #[test]
    fn negative_cache_subtree_invalidation_preserves_prefix_sibling() {
        let mut cache = super::NegativeDentryCache::new();
        cache.insert("/tmp/work");
        cache.insert("/tmp/work/output");
        cache.insert("/tmp/worker");

        assert_eq!(cache.remove_subtree("/tmp/work"), 2);
        assert!(!cache.contains("/tmp/work"));
        assert!(!cache.contains("/tmp/work/output"));
        assert!(cache.contains("/tmp/worker"));
    }

    #[test]
    fn positive_cache_publication_invalidates_matching_negative_entry() {
        let fs = AnotherExt4Fs::new();
        fs.negative_cache
          .lock()
          .get_or_insert_with(|| Box::new(super::NegativeDentryCache::new()))
          .insert("/created");
        fs.cache_insert("/created", 41);

        assert_eq!(fs.lookup_cache.lock().get("/created"), Some(&41));
        assert!(!fs.negative_cache.lock().as_ref().unwrap().contains("/created"));
    }
}

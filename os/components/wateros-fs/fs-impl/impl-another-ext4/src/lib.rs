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
use another_ext4::{
    Block, BlockDevice, ErrCode, Ext4, Ext4Error, FileType, InodeMode, BLOCK_SIZE, EXT4_ROOT_INO,
};
use api_v0::{
    FsAccessMode, FsCapability, FsDirEntry, FsError, FsImpl, FsKind, FsMetadata, FsNodeType,
    FsResult, LocalFs, LocalRwFs, ReadOnlyFs, ReadWriteFs, SharedFs, SharedRwFs,
};
use driver_block_api_v0::{Lba, SharedBlockDevice};
use spin::Mutex;

const EXT4_SUPER_MAGIC : u16 = 0xEF53;
const SUPERBLOCK_MAGIC_OFFSET : u64 = 1024 + 0x38;
const LOOKUP_CACHE_CAPACITY : usize = 4096;

fn map_error(error : Ext4Error) -> FsError {
    match error.code() {
        ErrCode::ENOENT => FsError::NotFound,
        ErrCode::EEXIST => FsError::Exists,
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
}

impl BlockDevice for BlockAdapter {
    fn read_block(&self, block_id : u64) -> Block {
        let mut data = Box::new([0u8; BLOCK_SIZE]);
        let mut guard = self.device.lock();
        let block_size = guard.block_size() as u64;
        if block_size == 0 || BLOCK_SIZE as u64 % block_size != 0 {
            panic!("another-ext4: unsupported device block size {block_size}");
        }
        guard.read_blocks(Lba(block_id * (BLOCK_SIZE as u64 / block_size)),
                          &mut data[..])
             .unwrap_or_else(|error| {
                 panic!("another-ext4: failed to read block {block_id}: {error:?}")
             });
        Block::new(block_id, data)
    }

    fn write_block(&self, block : &Block) {
        let mut guard = self.device.lock();
        let block_size = guard.block_size();
        if block_size == 0 || BLOCK_SIZE % block_size != 0 {
            panic!("another-ext4: unsupported device block size {block_size}");
        }
        let lba_count = BLOCK_SIZE / block_size;
        guard.write_blocks(Lba(block.id * lba_count as u64), &block.data[..])
             .unwrap_or_else(|error| {
                 panic!("another-ext4: failed to write block {}: {error:?}", block.id)
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

fn parent_name(path : &str) -> FsResult<(&str, &str)> {
    let path = path.trim_end_matches('/');
    let (parent, name) = path.rsplit_once('/').ok_or(FsError::InvalidPath)?;
    if name.is_empty() || name.len() > 255 || name == "." || name == ".." {
        return Err(FsError::InvalidPath);
    }
    Ok((if parent.is_empty() { "/" } else { parent }, name))
}

pub struct AnotherExt4Fs {
    fs : Option<Ext4>,
    lookup_cache : Mutex<BTreeMap<String, u32>>,
}

impl AnotherExt4Fs {
    const fn new() -> Self {
        Self { fs : None,
               lookup_cache : Mutex::new(BTreeMap::new()) }
    }
    fn get(&self) -> FsResult<&Ext4> {
        self.fs
            .as_ref()
            .ok_or(FsError::NotMounted)
    }

    fn get_mut(&mut self) -> FsResult<&mut Ext4> {
        self.fs.as_mut().ok_or(FsError::NotMounted)
    }

    fn lookup(&self, path : &str) -> FsResult<u32> {
        if let Some(inode) = self.lookup_cache.lock().get(path).copied() {
            return Ok(inode);
        }
        let inode = lookup(self.get()?, path)?;
        let mut cache = self.lookup_cache.lock();
        if cache.len() >= LOOKUP_CACHE_CAPACITY {
            cache.clear();
        }
        cache.insert(String::from(path), inode);
        Ok(inode)
    }

    fn cache_insert(&self, path : &str, inode : u32) {
        let mut cache = self.lookup_cache.lock();
        if cache.len() >= LOOKUP_CACHE_CAPACITY && !cache.contains_key(path) {
            cache.clear();
        }
        cache.insert(String::from(path), inode);
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
    }
}

impl ReadOnlyFs for AnotherExt4Fs {
    fn mount(&mut self, device : SharedBlockDevice) -> FsResult<()> {
        let backend = Arc::new(BlockAdapter { device });
        self.fs = Some(Ext4::load(backend).map_err(map_error)?);
        self.lookup_cache.lock().clear();
        Ok(())
    }

    fn is_mounted(&self) -> bool { self.fs.is_some() }

    fn exists(&self, path : &str) -> FsResult<bool> {
        match self.lookup(path) {
            Ok(_) => Ok(true),
            Err(FsError::NotFound) => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn metadata(&self, path : &str) -> FsResult<FsMetadata> {
        metadata(self.get()?, self.lookup(path)?)
    }

    fn read_range(&self, path : &str, offset : u64, buf : &mut [u8]) -> FsResult<usize> {
        let fs = self.get()?;
        let inode = self.lookup(path)?;
        fs.read(inode, offset as usize, buf).map_err(|error| {
            log::error!("[fs::another-ext4] read failed path={} inode={} offset={} len={} code={:?}",
                        path,
                        inode,
                        offset,
                        buf.len(),
                        error.code());
            map_error(error)
        })
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
        data.truncate(len);
        Ok(data)
    }
}

impl ReadWriteFs for AnotherExt4Fs {
    fn mount_rw(&mut self, device : SharedBlockDevice) -> FsResult<()> { self.mount(device) }
    fn is_mounted(&self) -> bool { self.fs.is_some() }

    fn sync(&mut self) -> FsResult<()> {
        self.get_mut()?.flush_all();
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
        fs.write(inode, 0, data).map_err(map_error)?;
        fs.flush_all();
        if created {
            self.cache_insert(path, inode);
        }
        Ok(())
    }

    fn unlink(&mut self, path : &str) -> FsResult<()> {
        let fs = self.get_mut()?;
        let inode = lookup(fs, path)?;
        if metadata(fs, inode)?.node_type == FsNodeType::Directory {
            return Err(FsError::NotAFile);
        }
        fs.generic_remove(EXT4_ROOT_INO, path).map_err(map_error)?;
        fs.flush_all();
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
        self.cache_remove_subtree(path);
        Ok(())
    }

    fn write_range(&mut self, path : &str, offset : u64, data : &[u8]) -> FsResult<usize> {
        let fs = self.get_mut()?;
        let inode = lookup(fs, path)?;
        fs.write(inode, offset as usize, data).map_err(map_error)
    }

    fn truncate(&mut self, path : &str, len : u64) -> FsResult<()> {
        let fs = self.get_mut()?;
        let inode = lookup(fs, path)?;
        fs.setattr(inode, None, None, None, Some(len), None, None, None, None)
          .map_err(map_error)?;
        fs.flush_all();
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
        Ok(())
    }

    fn chown(&mut self, path : &str, uid : Option<u32>, gid : Option<u32>) -> FsResult<()> {
        let fs = self.get_mut()?;
        let inode = lookup(fs, path)?;
        fs.setattr(inode, None, uid, gid, None, None, None, None, None)
          .map_err(map_error)?;
        fs.flush_all();
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
        self.cache_insert(path, inode);
        Ok(())
    }

    fn rename(&mut self, old_path : &str, new_path : &str) -> FsResult<()> {
        let fs = self.get_mut()?;
        fs.generic_rename(EXT4_ROOT_INO, old_path, new_path).map_err(map_error)?;
        fs.flush_all();
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
    use super::AnotherExt4Fs;

    #[test]
    fn lookup_cache_rename_moves_only_source_subtree() {
        let fs = AnotherExt4Fs::new();
        fs.cache_insert("/src", 10);
        fs.cache_insert("/src/child", 11);
        fs.cache_insert("/dst/stale", 12);
        fs.cache_insert("/unrelated", 13);

        fs.cache_rename_subtree("/src", "/dst");

        let cache = fs.lookup_cache.lock();
        assert_eq!(cache.get("/dst"), Some(&10));
        assert_eq!(cache.get("/dst/child"), Some(&11));
        assert_eq!(cache.get("/unrelated"), Some(&13));
        assert!(!cache.contains_key("/src"));
        assert!(!cache.contains_key("/src/child"));
        assert!(!cache.contains_key("/dst/stale"));
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
}

#![no_std]

//! WaterOS adapter for the vendored `another_ext4` implementation.
//!
//! The upstream crate works with fixed 4096-byte filesystem blocks and a
//! synchronous block-device trait.  This module keeps that detail behind the
//! stable WaterOS filesystem API.

extern crate alloc;

use alloc::boxed::Box;
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
            return Block::new(block_id, data);
        }
        let result = guard.read_blocks(Lba(block_id * (BLOCK_SIZE as u64 / block_size)),
                                       &mut data[..]);
        if result.is_err() {
            data.fill(0);
        }
        Block::new(block_id, data)
    }

    fn write_block(&self, block : &Block) {
        let mut guard = self.device.lock();
        let lba_count = BLOCK_SIZE / guard.block_size();
        let _ = guard.write_blocks(Lba(block.id * lba_count as u64),
                                   &block.data[..]);
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
    Ok(FsMetadata { node_type : map_type(attr.ftype),
                    size : attr.size,
                    mode : attr.perm.bits(),
                    inode : attr.ino as u64,
                    nlink : attr.links as u32,
                    uid : attr.uid,
                    gid : attr.gid })
}

fn parent_name(path : &str) -> FsResult<(&str, &str)> {
    let path = path.trim_end_matches('/');
    let (parent, name) = path.rsplit_once('/')
                             .ok_or(FsError::InvalidPath)?;
    if name.is_empty() || name.len() > 255 || name == "." || name == ".." {
        return Err(FsError::InvalidPath);
    }
    Ok((if parent.is_empty() { "/" } else { parent }, name))
}

pub struct AnotherExt4Fs {
    fs : Option<Ext4>,
}

impl AnotherExt4Fs {
    const fn new() -> Self { Self { fs : None } }
    fn get(&self) -> FsResult<&Ext4> {
        self.fs
            .as_ref()
            .ok_or(FsError::NotMounted)
    }
    fn get_mut(&mut self) -> FsResult<&mut Ext4> {
        self.fs
            .as_mut()
            .ok_or(FsError::NotMounted)
    }
}

impl ReadOnlyFs for AnotherExt4Fs {
    fn mount(&mut self, device : SharedBlockDevice) -> FsResult<()> {
        let backend = Arc::new(BlockAdapter { device });
        self.fs = Some(Ext4::load(backend).map_err(map_error)?);
        Ok(())
    }

    fn is_mounted(&self) -> bool { self.fs.is_some() }

    fn exists(&self, path : &str) -> FsResult<bool> {
        match lookup(self.get()?, path) {
            Ok(_) => Ok(true),
            Err(FsError::NotFound) => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn metadata(&self, path : &str) -> FsResult<FsMetadata> {
        metadata(self.get()?, lookup(self.get()?, path)?)
    }

    fn read_range(&self, path : &str, offset : u64, buf : &mut [u8]) -> FsResult<usize> {
        let fs = self.get()?;
        let inode = lookup(fs, path)?;
        let attr = fs.getattr(inode)
                     .map_err(map_error)?;
        if map_type(attr.ftype) != FsNodeType::File {
            return Err(FsError::NotAFile);
        }
        fs.read(inode, offset as usize, buf)
          .map_err(map_error)
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
        let inode = lookup(fs, path)?;
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
        let inode = lookup(fs, path)?;
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

    fn write_regular_file_at_root(&mut self, name : &str, data : &[u8]) -> FsResult<()> {
        let mut path = String::from("/");
        path.push_str(name);
        self.write_regular_file(&path, data)
    }

    fn write_regular_file(&mut self, path : &str, data : &[u8]) -> FsResult<()> {
        let fs = self.get_mut()?;
        let inode = match lookup(fs, path) {
            Ok(inode) => inode,
            Err(FsError::NotFound) => fs.generic_create(EXT4_ROOT_INO,
                                                        path,
                                                        InodeMode::FILE | InodeMode::ALL_RW)
                                        .map_err(map_error)?,
            Err(error) => return Err(error),
        };
        fs.setattr(inode,
                   None,
                   None,
                   None,
                   Some(0),
                   None,
                   None,
                   None,
                   None)
          .map_err(map_error)?;
        fs.write(inode, 0, data)
          .map_err(map_error)?;
        fs.flush_all();
        Ok(())
    }

    fn unlink(&mut self, path : &str) -> FsResult<()> {
        self.get_mut()?
            .generic_remove(EXT4_ROOT_INO, path)
            .map_err(map_error)
    }

    fn rmdir(&mut self, path : &str) -> FsResult<()> {
        self.get_mut()?
            .generic_remove(EXT4_ROOT_INO, path)
            .map_err(map_error)
    }

    fn write_range(&mut self, path : &str, offset : u64, data : &[u8]) -> FsResult<usize> {
        let fs = self.get_mut()?;
        let inode = lookup(fs, path)?;
        let written = fs.write(inode, offset as usize, data)
                        .map_err(map_error)?;
        fs.flush_all();
        Ok(written)
    }

    fn truncate(&mut self, path : &str, len : u64) -> FsResult<()> {
        let fs = self.get_mut()?;
        let inode = lookup(fs, path)?;
        fs.setattr(inode,
                   None,
                   None,
                   None,
                   Some(len),
                   None,
                   None,
                   None,
                   None)
          .map_err(map_error)
    }

    fn mkdir(&mut self, path : &str, mode : u32) -> FsResult<()> {
        let fs = self.get_mut()?;
        let (parent, name) = parent_name(path)?;
        let parent = lookup(fs, parent)?;
        fs.mkdir(parent,
                 name,
                 InodeMode::DIRECTORY | InodeMode::from_bits_retain(mode as u16))
          .map(|_| ())
          .map_err(map_error)
    }

    fn chmod(&mut self, path : &str, mode : u32) -> FsResult<()> {
        let fs = self.get_mut()?;
        let inode = lookup(fs, path)?;
        fs.setattr(inode,
                   Some(InodeMode::from_bits_retain(mode as u16)),
                   None,
                   None,
                   None,
                   None,
                   None,
                   None,
                   None)
          .map_err(map_error)
    }

    fn chown(&mut self, path : &str, uid : Option<u32>, gid : Option<u32>) -> FsResult<()> {
        let fs = self.get_mut()?;
        let inode = lookup(fs, path)?;
        fs.setattr(inode, None, uid, gid, None, None, None, None, None)
          .map_err(map_error)
    }

    fn rename(&mut self, old_path : &str, new_path : &str) -> FsResult<()> {
        self.get_mut()?
            .generic_rename(EXT4_ROOT_INO, old_path, new_path)
            .map_err(map_error)
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

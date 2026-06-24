#![no_std]

//! ext4 implementation backed by the `ext4_rs` crate.
//!
//! This crate intentionally mirrors the public [`api_v0::FsImpl`] surface used by
//! the existing ext4plus implementation, so the aggregate `wateros-fs` crate can
//! switch implementations through features.

extern crate alloc;

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use api_v0::{
    FsAccessMode, FsCapability, FsDirEntry, FsError, FsImpl, FsKind, FsMetadata, FsNodeType,
    FsResult, LocalFs, LocalRwFs, ReadOnlyFs, ReadWriteFs, SharedFs, SharedRwFs,
};
use driver_block_api_v0::{DriverError, Lba, SharedBlockDevice};
use ext4_rs::{BlockDevice as Ext4RsBlockDevice, Errno, Ext4, Ext4Error, InodeFileType};
use spin::Mutex;

const EXT4_SUPER_MAGIC : u16 = 0xEF53;
const SUPERBLOCK_OFFSET : u64 = 1024;
const MAGIC_OFFSET_IN_SB : usize = 0x38;
const ROOT_INODE : u32 = 2;
const S_IFREG : u16 = 0o100000;
const S_IFDIR : u16 = 0o040000;

fn probe_ext4_magic(device : &SharedBlockDevice) -> FsResult<bool> {
    let mut buf = [0u8; 2];
    device.lock()
          .read_bytes(SUPERBLOCK_OFFSET + MAGIC_OFFSET_IN_SB as u64,
                      &mut buf)
          .map_err(|_| FsError::Driver)?;
    Ok(u16::from_le_bytes(buf) == EXT4_SUPER_MAGIC)
}

struct BlockDevAdapter {
    device : SharedBlockDevice,
}

impl Ext4RsBlockDevice for BlockDevAdapter {
    fn read_offset(&self, offset : usize) -> Vec<u8> {
        let mut out = vec![0u8; ext4_rs::BLOCK_SIZE];
        let _ = self.device
                    .lock()
                    .read_bytes(offset as u64, &mut out);
        out
    }

    fn write_offset(&self, offset : usize, data : &[u8]) {
        let _ = block_write_bytes(&self.device, offset as u64, data);
    }
}

fn block_write_bytes(dev : &SharedBlockDevice,
                     start_byte : u64,
                     src : &[u8])
                     -> Result<(), DriverError> {
    if src.is_empty() {
        return Ok(());
    }
    let mut guard = dev.lock();
    let bdev : &mut dyn driver_block_api_v0::BlockDevice = &mut **guard;
    let bs = bdev.block_size();
    if bs == 0 {
        return Err(DriverError::InvalidParam);
    }
    let start = usize::try_from(start_byte).map_err(|_| DriverError::InvalidParam)?;
    let mut pos = 0usize;

    let head_off = start % bs;
    if head_off != 0 {
        let take = (bs - head_off).min(src.len());
        write_partial_block(bdev, start / bs, head_off, &src[..take])?;
        pos += take;
    }

    let full_bytes = ((src.len() - pos) / bs) * bs;
    if full_bytes > 0 {
        let block = (start + pos) / bs;
        bdev.write_blocks(Lba(block as u64),
                          &src[pos..pos + full_bytes])?;
        pos += full_bytes;
    }

    if pos < src.len() {
        let abs = start + pos;
        write_partial_block(bdev, abs / bs, 0, &src[pos..])?;
    }
    Ok(())
}

fn write_partial_block(bdev : &mut dyn driver_block_api_v0::BlockDevice,
                       block : usize,
                       offset : usize,
                       data : &[u8])
                       -> Result<(), DriverError> {
    let bs = bdev.block_size();
    if bs == 0 || offset >= bs || data.is_empty() || offset + data.len() > bs {
        return Err(DriverError::InvalidParam);
    }
    let mut block_buf = vec![0u8; bs];
    bdev.read_blocks(Lba(block as u64), &mut block_buf)?;
    block_buf[offset..offset + data.len()].copy_from_slice(data);
    bdev.write_blocks(Lba(block as u64), &block_buf)
}

fn map_ext4_rs(err : Ext4Error) -> FsError {
    match err.error() {
        Errno::ENOENT => FsError::NotFound,
        Errno::EEXIST => FsError::Exists,
        Errno::ENOTDIR | Errno::EISDIR => FsError::NotAFile,
        Errno::EINVAL | Errno::ENAMETOOLONG => FsError::InvalidPath,
        Errno::ENOSPC | Errno::EIO => FsError::Io,
        Errno::EROFS | Errno::ENOTSUP => FsError::Unsupported,
        _ => FsError::Io,
    }
}

fn map_node_type(kind : InodeFileType) -> FsNodeType {
    if kind == InodeFileType::S_IFDIR {
        FsNodeType::Directory
    } else if kind == InodeFileType::S_IFLNK {
        FsNodeType::Symlink
    } else if kind == InodeFileType::S_IFREG {
        FsNodeType::File
    } else {
        FsNodeType::Special
    }
}

fn split_parent_and_name(path : &str) -> FsResult<(&str, &str)> {
    let p = path.trim_end_matches('/');
    if p.is_empty() || p == "/" {
        return Err(FsError::InvalidPath);
    }
    let (parent, name) = p.rsplit_once('/')
                          .ok_or(FsError::InvalidPath)?;
    let parent = if parent.is_empty() { "/" } else { parent };
    if name.is_empty() || name.contains('/') {
        return Err(FsError::InvalidPath);
    }
    Ok((parent, name))
}

fn ensure_dir_inode(fs : &Ext4, inode : u32) -> FsResult<()> {
    let meta = metadata_for_inode(fs, inode)?;
    if meta.node_type == FsNodeType::Directory {
        Ok(())
    } else {
        Err(FsError::NotAFile)
    }
}

fn lookup_inode(fs : &Ext4, path : &str) -> FsResult<u32> {
    let p = path.trim_end_matches('/');
    if p.is_empty() || p == "/" {
        return Ok(ROOT_INODE);
    }

    let mut inode = ROOT_INODE;
    let mut parts = p.split('/')
                     .filter(|part| !part.is_empty())
                     .peekable();
    while let Some(part) = parts.next() {
        if part == "." || part == ".." || part.len() > 255 {
            return Err(FsError::InvalidPath);
        }
        let attr = fs.fuse_lookup(inode as u64, part)
                     .map_err(map_ext4_rs)?;
        if parts.peek()
                .is_some() &&
           attr.kind != InodeFileType::S_IFDIR
        {
            return Err(FsError::NotAFile);
        }
        inode = u32::try_from(attr.ino).map_err(|_| FsError::Io)?;
    }
    Ok(inode)
}

fn metadata_for_inode(fs : &Ext4, inode : u32) -> FsResult<FsMetadata> {
    let attr = fs.fuse_getattr(inode as u64)
                 .map_err(map_ext4_rs)?;
    Ok(FsMetadata { node_type : map_node_type(attr.kind),
                    size : attr.size,
                    mode : attr.kind.bits() | attr.perm.bits(),
                    inode : attr.ino,
                    nlink : attr.nlink })
}

fn walk_ext4_rs_tree(fs : &Ext4RsFs, path : &str) {
    let Ok(entries) = ReadOnlyFs::read_dir(fs, path) else {
        return;
    };
    for entry in entries {
        let child = if path == "/" {
            format!("/{}", entry.name)
        } else {
            format!("{}/{}",
                    path.trim_end_matches('/'),
                    entry.name)
        };
        log::trace!("[fs::boot-tree] {}", child);
        if entry.node_type == FsNodeType::Directory {
            walk_ext4_rs_tree(fs, child.as_str());
        }
    }
}

fn create_regular(fs : &mut Ext4, path : &str, mode : u16) -> FsResult<u32> {
    let (parent_path, name) = split_parent_and_name(path)?;
    let parent = lookup_inode(fs, parent_path)?;
    ensure_dir_inode(fs, parent)?;
    fs.fuse_mknod(parent as u64,
                  name,
                  u32::from(mode),
                  0,
                  0)
      .map_err(map_ext4_rs)?;
    fs.fuse_lookup(parent as u64, name)
      .map(|attr| attr.ino as u32)
      .map_err(map_ext4_rs)
}

pub struct Ext4RsFs {
    fs : Option<Ext4>,
}

impl Ext4RsFs {
    pub const fn new() -> Self { Self { fs : None } }

    fn fs(&self) -> FsResult<&Ext4> {
        self.fs
            .as_ref()
            .ok_or(FsError::NotMounted)
    }

    fn fs_mut(&mut self) -> FsResult<&mut Ext4> {
        self.fs
            .as_mut()
            .ok_or(FsError::NotMounted)
    }
}

impl ReadOnlyFs for Ext4RsFs {
    fn mount(&mut self, device : SharedBlockDevice) -> FsResult<()> {
        let dev : Arc<dyn Ext4RsBlockDevice> = Arc::new(BlockDevAdapter { device });
        self.fs = Some(Ext4::open(dev));
        Ok(())
    }

    fn is_mounted(&self) -> bool { self.fs.is_some() }

    fn exists(&self, path : &str) -> FsResult<bool> {
        match lookup_inode(self.fs()?, path) {
            Ok(_) => Ok(true),
            Err(FsError::NotFound) => Ok(false),
            Err(err) => Err(err),
        }
    }

    fn metadata(&self, path : &str) -> FsResult<FsMetadata> {
        let inode = lookup_inode(self.fs()?, path)?;
        metadata_for_inode(self.fs()?, inode)
    }

    fn read(&self, path : &str) -> FsResult<Vec<u8>> {
        let meta = ReadOnlyFs::metadata(self, path)?;
        if meta.node_type != FsNodeType::File {
            return Err(FsError::NotAFile);
        }
        let mut out = vec![0u8; usize::try_from(meta.size).map_err(|_| FsError::Io)?];
        let n = ReadOnlyFs::read_range(self, path, 0, &mut out)?;
        if n != out.len() {
            return Err(FsError::Io);
        }
        Ok(out)
    }

    fn read_range(&self, path : &str, offset : u64, buf : &mut [u8]) -> FsResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let fs = self.fs()?;
        let meta = ReadOnlyFs::metadata(self, path)?;
        if meta.node_type != FsNodeType::File {
            return Err(FsError::NotAFile);
        }
        if offset >= meta.size {
            return Ok(0);
        }
        let inode = u32::try_from(meta.inode).map_err(|_| FsError::Io)?;
        let to_read = buf.len()
                         .min(usize::try_from(meta.size - offset).map_err(|_| FsError::Io)?);
        fs.read_at(inode,
                   usize::try_from(offset).map_err(|_| FsError::Io)?,
                   &mut buf[..to_read])
          .map_err(map_ext4_rs)
    }

    fn read_prefix(&self, path : &str, len : usize) -> FsResult<Vec<u8>> {
        let mut buf = vec![0u8; len];
        let n = ReadOnlyFs::read_range(self, path, 0, &mut buf)?;
        buf.truncate(n);
        Ok(buf)
    }

    fn read_dir(&self, path : &str) -> FsResult<Vec<FsDirEntry>> {
        let fs = self.fs()?;
        let inode = lookup_inode(fs, path)?;
        let attr = fs.fuse_getattr(inode as u64)
                     .map_err(map_ext4_rs)?;
        if attr.kind != InodeFileType::S_IFDIR {
            return Err(FsError::NotAFile);
        }
        let mut out = Vec::new();
        for entry in fs.ext4_dir_get_entries(inode) {
            if entry.unused() {
                continue;
            }
            let name = entry.get_name();
            if name == "." || name == ".." {
                continue;
            }
            let child = fs.fuse_getattr(entry.inode as u64)
                          .map_err(map_ext4_rs)?;
            out.push(FsDirEntry { name,
                                  node_type : map_node_type(child.kind) });
        }
        Ok(out)
    }

    fn boot_dump_all_paths(&self) {
        if self.fs.is_some() {
            walk_ext4_rs_tree(self, "/");
        }
    }
}

impl ReadWriteFs for Ext4RsFs {
    fn mount_rw(&mut self, device : SharedBlockDevice) -> FsResult<()> { self.mount(device) }

    fn is_mounted(&self) -> bool { self.fs.is_some() }

    fn write_regular_file_at_root(&mut self, name : &str, data : &[u8]) -> FsResult<()> {
        if name.is_empty() || name.contains('/') {
            return Err(FsError::InvalidPath);
        }
        let mut path = String::from("/");
        path.push_str(name);
        self.write_regular_file(path.as_str(), data)
    }

    fn write_regular_file(&mut self, path : &str, data : &[u8]) -> FsResult<()> {
        let fs = self.fs_mut()?;
        let inode = match lookup_inode(fs, path) {
            Ok(inode) => {
                let meta = metadata_for_inode(fs, inode)?;
                if meta.node_type != FsNodeType::File {
                    return Err(FsError::NotAFile);
                }
                if meta.size > 0 {
                    let mut inode_ref = fs.get_inode_ref(inode);
                    fs.truncate_inode(&mut inode_ref, 0)
                      .map_err(map_ext4_rs)?;
                }
                inode
            }
            Err(FsError::NotFound) => create_regular(fs, path, S_IFREG | 0o644)?,
            Err(err) => return Err(err),
        };
        if !data.is_empty() {
            write_all(fs, inode, 0, data)?;
        }
        Ok(())
    }

    fn unlink(&mut self, path : &str) -> FsResult<()> {
        let fs = self.fs_mut()?;
        let (parent_path, name) = split_parent_and_name(path)?;
        let parent = lookup_inode(fs, parent_path)?;
        ensure_dir_inode(fs, parent)?;
        let attr = fs.fuse_lookup(parent as u64, name)
                     .map_err(map_ext4_rs)?;
        if attr.kind == InodeFileType::S_IFDIR {
            return Err(FsError::NotAFile);
        }
        if attr.kind != InodeFileType::S_IFREG {
            return Err(FsError::Unsupported);
        }

        let mut parent_ref = fs.get_inode_ref(parent);
        let child = u32::try_from(attr.ino).map_err(|_| FsError::Io)?;
        let mut child_ref = fs.get_inode_ref(child);
        fs.dir_remove_entry(&mut parent_ref, name)
          .map_err(map_ext4_rs)?;

        let links = child_ref.inode
                             .links_count();
        if links <= 1 {
            if child_ref.inode
                        .size() >
               0
            {
                fs.truncate_inode(&mut child_ref, 0)
                  .map_err(map_ext4_rs)?;
            }
            fs.ialloc_free_inode(child_ref.inode_num, false);
        } else {
            child_ref.inode
                     .set_links_count(links - 1);
            fs.write_back_inode(&mut child_ref);
        }
        fs.write_back_inode(&mut parent_ref);
        Ok(())
    }

    fn rmdir(&mut self, path : &str) -> FsResult<()> {
        if !ReadOnlyFs::read_dir(self, path)?.is_empty() {
            return Err(FsError::Exists);
        }
        let fs = self.fs_mut()?;
        let (parent_path, name) = split_parent_and_name(path)?;
        let parent = lookup_inode(fs, parent_path)?;
        fs.fuse_rmdir(parent as u64, name)
          .map_err(map_ext4_rs)?;
        Ok(())
    }

    fn write_range(&mut self, path : &str, offset : u64, data : &[u8]) -> FsResult<usize> {
        if data.is_empty() {
            return Ok(0);
        }
        let fs = self.fs_mut()?;
        let inode = lookup_inode(fs, path)?;
        let meta = metadata_for_inode(fs, inode)?;
        if meta.node_type != FsNodeType::File {
            return Err(FsError::NotAFile);
        }
        write_all(fs, inode, offset, data)?;
        Ok(data.len())
    }

    fn truncate(&mut self, path : &str, len : u64) -> FsResult<()> {
        let fs = self.fs_mut()?;
        let inode = lookup_inode(fs, path)?;
        let meta = metadata_for_inode(fs, inode)?;
        if meta.node_type != FsNodeType::File {
            return Err(FsError::NotAFile);
        }
        if len > meta.size {
            zero_extend_file(fs, inode, meta.size, len)?;
        } else if len < meta.size {
            let mut inode_ref = fs.get_inode_ref(inode);
            fs.truncate_inode(&mut inode_ref, len)
              .map_err(map_ext4_rs)?;
        }
        Ok(())
    }

    fn mkdir(&mut self, path : &str, mode : u32) -> FsResult<()> {
        let fs = self.fs_mut()?;
        let (parent_path, name) = split_parent_and_name(path)?;
        let parent = lookup_inode(fs, parent_path)?;
        ensure_dir_inode(fs, parent)?;
        let mode = S_IFDIR | (mode as u16 & 0o7777);
        fs.fuse_mkdir(parent as u64, name, u32::from(mode), 0)
          .map_err(map_ext4_rs)?;
        Ok(())
    }

    fn chmod(&mut self, path : &str, mode : u32) -> FsResult<()> {
        let fs = self.fs_mut()?;
        let inode = lookup_inode(fs, path)?;
        let meta = metadata_for_inode(fs, inode)?;
        fs.fuse_setattr(inode as u64,
                        Some(u32::from(meta.mode & !0o7777) | (mode & 0o7777)),
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None);
        Ok(())
    }

    fn chown(&mut self, path : &str, uid : Option<u32>, gid : Option<u32>) -> FsResult<()> {
        if uid.is_none() && gid.is_none() {
            return ReadOnlyFs::metadata(self, path).map(|_| ());
        }
        let fs = self.fs_mut()?;
        let inode = lookup_inode(fs, path)?;
        fs.fuse_setattr(inode as u64,
                        None,
                        uid,
                        gid,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None,
                        None);
        Ok(())
    }

    fn hardlink(&mut self, existing_path : &str, new_path : &str) -> FsResult<()> {
        let fs = self.fs_mut()?;
        let existing = lookup_inode(fs, existing_path)?;
        let meta = metadata_for_inode(fs, existing)?;
        if meta.node_type == FsNodeType::Directory {
            return Err(FsError::NotAFile);
        }
        if meta.node_type != FsNodeType::File {
            return Err(FsError::Unsupported);
        }

        let (parent_path, name) = split_parent_and_name(new_path)?;
        let parent = lookup_inode(fs, parent_path)?;
        ensure_dir_inode(fs, parent)?;
        match fs.fuse_lookup(parent as u64, name) {
            Ok(_) => return Err(FsError::Exists),
            Err(err) if map_ext4_rs(err) == FsError::NotFound => {}
            Err(err) => return Err(map_ext4_rs(err)),
        }

        let mut parent_ref = fs.get_inode_ref(parent);
        let mut child_ref = fs.get_inode_ref(existing);
        fs.link(&mut parent_ref, &mut child_ref, name)
          .map_err(map_ext4_rs)?;
        fs.write_back_inode(&mut child_ref);
        fs.write_back_inode(&mut parent_ref);
        Ok(())
    }

    fn rename(&mut self, old_path : &str, new_path : &str) -> FsResult<()> {
        let (old_parent_path, old_name) = split_parent_and_name(old_path)?;
        let (new_parent_path, new_name) = split_parent_and_name(new_path)?;
        if old_parent_path != new_parent_path {
            return Err(FsError::Unsupported);
        }

        let fs = self.fs_mut()?;
        let parent = lookup_inode(fs, old_parent_path)?;
        ensure_dir_inode(fs, parent)?;
        let old_attr = fs.fuse_lookup(parent as u64, old_name)
                         .map_err(map_ext4_rs)?;
        match fs.fuse_lookup(parent as u64, new_name) {
            Ok(_) => return Err(FsError::Exists),
            Err(err) if map_ext4_rs(err) == FsError::NotFound => {}
            Err(err) => return Err(map_ext4_rs(err)),
        }

        let child = u32::try_from(old_attr.ino).map_err(|_| FsError::Io)?;
        let mut parent_ref = fs.get_inode_ref(parent);
        let child_ref = fs.get_inode_ref(child);
        fs.dir_add_entry(&mut parent_ref, &child_ref, new_name)
          .map_err(map_ext4_rs)?;
        fs.dir_remove_entry(&mut parent_ref, old_name)
          .map_err(map_ext4_rs)?;
        fs.write_back_inode(&mut parent_ref);
        Ok(())
    }

    fn exists(&self, path : &str) -> FsResult<bool> { ReadOnlyFs::exists(self, path) }

    fn metadata(&self, path : &str) -> FsResult<FsMetadata> { ReadOnlyFs::metadata(self, path) }

    fn read(&self, path : &str) -> FsResult<Vec<u8>> { ReadOnlyFs::read(self, path) }

    fn read_range(&self, path : &str, offset : u64, buf : &mut [u8]) -> FsResult<usize> {
        ReadOnlyFs::read_range(self, path, offset, buf)
    }

    fn read_dir(&self, path : &str) -> FsResult<Vec<FsDirEntry>> {
        ReadOnlyFs::read_dir(self, path)
    }
}

fn write_all(fs : &Ext4, inode : u32, offset : u64, data : &[u8]) -> FsResult<()> {
    let mut done = 0usize;
    while done < data.len() {
        let n = fs.write_at(inode,
                            usize::try_from(offset + done as u64).map_err(|_| FsError::Io)?,
                            &data[done..])
                  .map_err(map_ext4_rs)?;
        if n == 0 {
            return Err(FsError::Io);
        }
        done = done.checked_add(n)
                   .ok_or(FsError::Io)?;
    }
    Ok(())
}

fn zero_extend_file(fs : &Ext4, inode : u32, old_size : u64, new_size : u64) -> FsResult<()> {
    let zeroes = [0u8; ext4_rs::BLOCK_SIZE];
    let mut offset = old_size;
    while offset < new_size {
        let len =
            usize::try_from((new_size - offset).min(zeroes.len() as u64)).map_err(|_| FsError::Io)?;
        write_all(fs, inode, offset, &zeroes[..len])?;
        offset = offset.checked_add(len as u64)
                       .ok_or(FsError::Io)?;
    }
    Ok(())
}

pub struct Ext4RsImpl;

pub static IMPL : Ext4RsImpl = Ext4RsImpl;

const SUPPORTED : &[FsCapability] = &[FsCapability::new(FsKind::Ext4, FsAccessMode::ReadOnly),
                                      FsCapability::new(FsKind::Ext4, FsAccessMode::ReadWrite)];

impl FsImpl for Ext4RsImpl {
    fn name(&self) -> &'static str { "ext4-rs" }

    fn supported(&self) -> &'static [FsCapability] { SUPPORTED }

    fn probe(&self, device : &SharedBlockDevice) -> FsResult<Option<FsKind>> {
        if probe_ext4_magic(device)? {
            Ok(Some(FsKind::Ext4))
        } else {
            Ok(None)
        }
    }

    fn mount_ro(&self, device : SharedBlockDevice) -> FsResult<SharedFs> {
        log::info!("[fs::ext4-rs] mount_ro begin");
        let mut fs = Ext4RsFs::new();
        ReadOnlyFs::mount(&mut fs, device)?;
        Ok(Arc::new(Mutex::new(LocalFs::new(Box::new(fs)))))
    }

    fn mount_rw(&self, device : SharedBlockDevice) -> FsResult<SharedRwFs> {
        log::info!("[fs::ext4-rs] mount_rw begin");
        let mut fs = Ext4RsFs::new();
        ReadWriteFs::mount_rw(&mut fs, device)?;
        Ok(Arc::new(Mutex::new(LocalRwFs::new(Box::new(fs)))))
    }
}

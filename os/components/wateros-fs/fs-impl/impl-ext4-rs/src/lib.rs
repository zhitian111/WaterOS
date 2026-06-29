#![no_std]
//! 本模块代码由AI完成

//! 基于 `ext4_rs` crate 的 ext4 实现。
//!
//! 对外 [`api_v0::FsImpl`] 面与旧 `impl-ext4`（ext4plus）对齐，供 `wateros-fs` 通过 feature 切换。
//! RW 路径依赖 ext4_rs 块分配语义，跨 EOF 写前须显式补洞（见 [`ReadWriteFs::write_range`] 内注释）。

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

/// ext2/3/4 共用的 superblock magic（Linux 布局 `s_magic = 0xEF53`）。
// 本变量代码由AI完成
const EXT4_SUPER_MAGIC : u16 = 0xEF53;
/// 主 superblock 起始字节偏移（卷头 1024 字节之后）。
// 本变量代码由AI完成
const SUPERBLOCK_OFFSET : u64 = 1024;
/// `s_magic` 在 1024 字节 superblock 内的偏移。
// 本变量代码由AI完成
const MAGIC_OFFSET_IN_SB : usize = 0x38;
/// ext4 根目录 inode 号（固定为 2）。
// 本变量代码由AI完成
const ROOT_INODE : u32 = 2;
/// 普通文件 mode 前缀（`S_IFREG`）。
// 本变量代码由AI完成
const S_IFREG : u16 = 0o100000;
/// 目录 mode 前缀（`S_IFDIR`）。
// 本变量代码由AI完成
const S_IFDIR : u16 = 0o040000;

// 读取 superblock magic，判定是否为 ext2/3/4 卷。
// 本方法代码由AI完成
fn probe_ext4_magic(device : &SharedBlockDevice) -> FsResult<bool> {
    let mut buf = [0u8; 2];
    device.lock()
          .read_bytes(SUPERBLOCK_OFFSET + MAGIC_OFFSET_IN_SB as u64,
                      &mut buf)
          .map_err(|_| FsError::Driver)?;
    Ok(u16::from_le_bytes(buf) == EXT4_SUPER_MAGIC)
}

// 将 [`SharedBlockDevice`] 适配为 ext4_rs 的 [`Ext4RsBlockDevice`]。
// 本结构代码由AI完成
struct BlockDevAdapter {
    device : SharedBlockDevice,
}

impl Ext4RsBlockDevice for BlockDevAdapter {
// 本方法代码由AI完成
    fn read_offset(&self, offset : usize) -> Vec<u8> {
        let mut out = vec![0u8; ext4_rs::BLOCK_SIZE];
        let _ = self.device
                    .lock()
                    .read_bytes(offset as u64, &mut out);
        out
    }

// 本方法代码由AI完成
    fn write_offset(&self, offset : usize, data : &[u8]) {
        let _ = block_write_bytes(&self.device, offset as u64, data);
    }
}

// 按字节写入块设备：头尾非块对齐时读-改-写，中间整块直接写。
// 本方法代码由AI完成
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

// 单块内部分写入：先读整块再 patch 子区间。
// 本方法代码由AI完成
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

// 将 ext4_rs  errno 映射为公共 [`FsError`]。
// 本方法代码由AI完成
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

// ext4_rs inode 类型 → API 层 [`FsNodeType`]。
// 本方法代码由AI完成
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

// 拆分绝对路径为 (父目录, 末级名字)；根或空名返回 [`FsError::InvalidPath`]。
// 本方法代码由AI完成
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

// 确认 inode 为目录，否则返回 [`FsError::NotAFile`]。
// 本方法代码由AI完成
fn ensure_dir_inode(fs : &Ext4, inode : u32) -> FsResult<()> {
    let meta = metadata_for_inode(fs, inode)?;
    if meta.node_type == FsNodeType::Directory {
        Ok(())
    } else {
        Err(FsError::NotAFile)
    }
}

// 按路径逐级 fuse_lookup，返回末级 inode 号。
// 本方法代码由AI完成
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

// 由 inode 号构造 [`FsMetadata`] 快照。
// 本方法代码由AI完成
fn metadata_for_inode(fs : &Ext4, inode : u32) -> FsResult<FsMetadata> {
    let inode_ref = fs.get_inode_ref(inode);
    Ok(FsMetadata { node_type : map_node_type(inode_ref.inode
                                                       .file_type()),
                    size : inode_ref.inode
                                    .size(),
                    mode : inode_ref.inode
                                    .mode(),
                    inode : inode_ref.inode_num as u64,
                    nlink : inode_ref.inode
                                     .links_count() as u32,
                    uid : inode_ref.inode.uid() as u32,
                    gid : inode_ref.inode.gid() as u32 })
}

// 启动期 DFS 打印 ext4 目录树（trace 级别）。
// 本方法代码由AI完成
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

// 在父目录下创建普通文件 inode 并返回其编号。
// 本方法代码由AI完成
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

// 创建目录 inode；已存在同名项返回 [`FsError::Exists`]。
// 本方法代码由AI完成
fn create_directory(fs : &mut Ext4, path : &str, mode : u16) -> FsResult<()> {
    let (parent_path, name) = split_parent_and_name(path)?;
    let parent = lookup_inode(fs, parent_path)?;
    ensure_dir_inode(fs, parent)?;
    match fs.fuse_lookup(parent as u64, name) {
        Ok(_) => return Err(FsError::Exists),
        Err(err) if map_ext4_rs(err) == FsError::NotFound => {}
        Err(err) => return Err(map_ext4_rs(err)),
    }
    let mut inode_ref = fs.create(parent, name, mode)
                          .map_err(map_ext4_rs)?;
    inode_ref.inode
             .set_mode(mode);
    fs.write_back_inode(&mut inode_ref);
    Ok(())
}

/// ext4_rs 卷句柄；挂载成功后内部持有 [`Ext4`] 实例。
// 本结构代码由AI完成
pub struct Ext4RsFs {
    fs : Option<Ext4>,
}

impl Ext4RsFs {
    /// 构造未挂载句柄。
    pub const fn new() -> Self { Self { fs : None } }

// 本方法代码由AI完成
    fn fs(&self) -> FsResult<&Ext4> {
        self.fs
            .as_ref()
            .ok_or(FsError::NotMounted)
    }

// 本方法代码由AI完成
    fn fs_mut(&mut self) -> FsResult<&mut Ext4> {
        self.fs
            .as_mut()
            .ok_or(FsError::NotMounted)
    }
}

impl ReadOnlyFs for Ext4RsFs {
// 本方法代码由AI完成
    fn mount(&mut self, device : SharedBlockDevice) -> FsResult<()> {
        let dev : Arc<dyn Ext4RsBlockDevice> = Arc::new(BlockDevAdapter { device });
        self.fs = Some(Ext4::open(dev));
        Ok(())
    }

    fn is_mounted(&self) -> bool { self.fs.is_some() }

// 本方法代码由AI完成
    fn exists(&self, path : &str) -> FsResult<bool> {
        match lookup_inode(self.fs()?, path) {
            Ok(_) => Ok(true),
            Err(FsError::NotFound) => Ok(false),
            Err(err) => Err(err),
        }
    }

// 本方法代码由AI完成
    fn metadata(&self, path : &str) -> FsResult<FsMetadata> {
        let inode = lookup_inode(self.fs()?, path)?;
        metadata_for_inode(self.fs()?, inode)
    }

// 本方法代码由AI完成
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

// 本方法代码由AI完成
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

// 本方法代码由AI完成
    fn read_prefix(&self, path : &str, len : usize) -> FsResult<Vec<u8>> {
        let mut buf = vec![0u8; len];
        let n = ReadOnlyFs::read_range(self, path, 0, &mut buf)?;
        buf.truncate(n);
        Ok(buf)
    }

// 本方法代码由AI完成
    fn read_dir(&self, path : &str) -> FsResult<Vec<FsDirEntry>> {
        let fs = self.fs()?;
        let inode = lookup_inode(fs, path)?;
        let attr = fs.fuse_getattr(inode as u64)
                     .map_err(map_ext4_rs)?;
        if attr.kind != InodeFileType::S_IFDIR {
            return Err(FsError::NotAFile);
        }
        let mut out = Vec::new();
        for entry in fs.ext4_dir_get_entries(inode)
                        .map_err(map_ext4_rs)?
        {
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

// 本方法代码由AI完成
    fn read_symlink(&self, path : &str) -> FsResult<Vec<u8>> {
        let fs = self.fs()?;
        let inode = lookup_inode(fs, path)?;
        let meta = metadata_for_inode(fs, inode)?;
        if meta.node_type != FsNodeType::Symlink {
            return Err(FsError::NotAFile);
        }
        let inode_ref = fs.get_inode_ref(inode);
        let file_size = inode_ref.inode.size();
        let mut read_buf = vec![0u8; file_size as usize];
        if file_size > 0 {
            fs.read_at(inode, 0, &mut read_buf).map_err(map_ext4_rs)?;
        }
        Ok(read_buf)
    }

// 本方法代码由AI完成
    fn boot_dump_all_paths(&self) {
        if self.fs.is_some() {
            walk_ext4_rs_tree(self, "/");
        }
    }
}

impl ReadWriteFs for Ext4RsFs {
    fn mount_rw(&mut self, device : SharedBlockDevice) -> FsResult<()> { self.mount(device) }

    fn is_mounted(&self) -> bool { self.fs.is_some() }

// 本方法代码由AI完成
    fn write_regular_file_at_root(&mut self, name : &str, data : &[u8]) -> FsResult<()> {
        if name.is_empty() || name.contains('/') {
            return Err(FsError::InvalidPath);
        }
        let mut path = String::from("/");
        path.push_str(name);
        self.write_regular_file(path.as_str(), data)
    }

// 本方法代码由AI完成
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

// 本方法代码由AI完成
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
        if attr.kind != InodeFileType::S_IFREG
            && attr.kind != InodeFileType::S_IFLNK
            && attr.kind != InodeFileType::S_IFSOCK
        {
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

// 本方法代码由AI完成
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

// 本方法代码由AI完成
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
        // 底层 ext4_rs 的块分配是纯追加式（新块逻辑号恒为 ceil(size/bs)），无法在
        // offset > 当前文件大小处直接写出空洞。页缓存的逐页驱逐 / 分段 flush 可能
        // 在低页尚未落盘时先写回高页，从而对 ext4 发起越过 EOF 的写；若不先补洞，
        // 数据会落到错误的逻辑块，回读命中空洞返回 0。这里显式把 [size, offset)
        // 补成真实零块，保证写入落到正确逻辑块。
        if offset > meta.size {
            zero_extend_file(fs, inode, meta.size, offset)?;
        }
        write_all(fs, inode, offset, data)?;
        Ok(data.len())
    }

// 本方法代码由AI完成
    fn truncate(&mut self, path : &str, len : u64) -> FsResult<()> {
        let fs = self.fs_mut()?;
        let inode = lookup_inode(fs, path)?;
        let meta = metadata_for_inode(fs, inode)?;
        if meta.node_type != FsNodeType::File {
            return Err(FsError::NotAFile);
        }
        if len > meta.size {
            // fallocate(2) / truncate(2) 扩文件只需稀疏扩展逻辑长度，与 Linux
            // 及 ext4plus 一致；勿逐块写零（LTP mount_device 会预分配 300MB）。
            let mut inode_ref = fs.get_inode_ref(inode);
            inode_ref.inode
                     .set_size(len);
            fs.write_back_inode(&mut inode_ref);
        } else if len < meta.size {
            let mut inode_ref = fs.get_inode_ref(inode);
            fs.truncate_inode(&mut inode_ref, len)
              .map_err(map_ext4_rs)?;
        }
        Ok(())
    }

// 本方法代码由AI完成
    fn mkdir(&mut self, path : &str, mode : u32) -> FsResult<()> {
        let fs = self.fs_mut()?;
        let mode = S_IFDIR | (mode as u16 & 0o7777);
        create_directory(fs, path, mode)
    }

// 本方法代码由AI完成
    fn chmod(&mut self, path : &str, mode : u32) -> FsResult<()> {
        let fs = self.fs_mut()?;
        let inode = lookup_inode(fs, path)?;
        let meta = metadata_for_inode(fs, inode)?;
        let mut inode_ref = fs.get_inode_ref(inode);
        inode_ref.inode
                 .set_mode((meta.mode & !0o7777) | (mode as u16 & 0o7777));
        fs.write_back_inode(&mut inode_ref);
        Ok(())
    }

// 本方法代码由AI完成
    fn chown(&mut self, path : &str, uid : Option<u32>, gid : Option<u32>) -> FsResult<()> {
        if uid.is_none() && gid.is_none() {
            return ReadOnlyFs::metadata(self, path).map(|_| ());
        }
        let fs = self.fs_mut()?;
        let inode = lookup_inode(fs, path)?;
        let mut inode_ref = fs.get_inode_ref(inode);
        if let Some(uid) = uid {
            inode_ref.inode
                     .set_uid(uid as u16);
        }
        if let Some(gid) = gid {
            inode_ref.inode
                     .set_gid(gid as u16);
        }
        fs.write_back_inode(&mut inode_ref);
        Ok(())
    }

// 本方法代码由AI完成
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

// 本方法代码由AI完成
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
        let mut child_ref = fs.get_inode_ref(child);
        if child_ref.inode.is_dir() {
            fs.dir_add_entry(&mut parent_ref, &child_ref, new_name)
              .map_err(map_ext4_rs)?;
            fs.dir_remove_entry(&mut parent_ref, old_name)
              .map_err(map_ext4_rs)?;
        } else {
            fs.link(&mut parent_ref, &mut child_ref, new_name)
              .map_err(map_ext4_rs)?;
            fs.dir_remove_entry(&mut parent_ref, old_name)
              .map_err(map_ext4_rs)?;
            fs.write_back_inode(&mut child_ref);
        }
        fs.write_back_inode(&mut parent_ref);
        Ok(())
    }

// 本方法代码由AI完成
    fn symlink(&mut self, link_path : &str, target : &str) -> FsResult<()> {
        let fs = self.fs_mut()?;
        let (parent_path, name) = split_parent_and_name(link_path)?;
        let parent = lookup_inode(fs, parent_path)?;
        ensure_dir_inode(fs, parent)?;
        fs.fuse_symlink(parent as u64, name, target)
          .map_err(map_ext4_rs)?;
        let attr = fs.fuse_lookup(parent as u64, name)
                     .map_err(map_ext4_rs)?;
        let inode = u32::try_from(attr.ino).map_err(|_| FsError::Io)?;
        if !target.is_empty() {
            write_all(fs, inode, 0, target.as_bytes())?;
        }
        Ok(())
    }

// 本方法代码由AI完成
    fn mknod(&mut self, path : &str, mode : u32, rdev : u32) -> FsResult<()> {
        let fs = self.fs_mut()?;
        let (parent_path, name) = split_parent_and_name(path)?;
        let parent = lookup_inode(fs, parent_path)?;
        ensure_dir_inode(fs, parent)?;
        match fs.fuse_lookup(parent as u64, name) {
            Ok(_) => return Err(FsError::Exists),
            Err(err) if map_ext4_rs(err) == FsError::NotFound => {}
            Err(err) => return Err(map_ext4_rs(err)),
        }
        fs.fuse_mknod(parent as u64, name, mode, 0, rdev)
          .map_err(map_ext4_rs)?;
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

    fn read_symlink(&self, path : &str) -> FsResult<Vec<u8>> {
        ReadOnlyFs::read_symlink(self, path)
    }
}

// 从 offset 起分片写入，直至 data 全部落盘。
// 本方法代码由AI完成
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

// 将 [old_size, new_size) 区间用零块填满，供跨 EOF 写前补洞。
// 本方法代码由AI完成
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

/// ext4_rs 的 [`FsImpl`] 注册类型。
// 本结构代码由AI完成
pub struct Ext4RsImpl;

/// 全局 ext4-rs impl 实例。
// 本变量代码由AI完成
pub static IMPL : Ext4RsImpl = Ext4RsImpl;

// 本变量代码由AI完成
const SUPPORTED : &[FsCapability] = &[FsCapability::new(FsKind::Ext4, FsAccessMode::ReadOnly),
                                      FsCapability::new(FsKind::Ext4, FsAccessMode::ReadWrite)];

impl FsImpl for Ext4RsImpl {
    fn name(&self) -> &'static str { "ext4-rs" }

    fn supported(&self) -> &'static [FsCapability] { SUPPORTED }

// 本方法代码由AI完成
    fn probe(&self, device : &SharedBlockDevice) -> FsResult<Option<FsKind>> {
        if probe_ext4_magic(device)? {
            Ok(Some(FsKind::Ext4))
        } else {
            Ok(None)
        }
    }

// 本方法代码由AI完成
    fn mount_ro(&self, device : SharedBlockDevice) -> FsResult<SharedFs> {
        log::info!("[fs::ext4-rs] mount_ro begin");
        let mut fs = Ext4RsFs::new();
        ReadOnlyFs::mount(&mut fs, device)?;
        Ok(Arc::new(Mutex::new(LocalFs::new(Box::new(fs)))))
    }

// 本方法代码由AI完成
    fn mount_rw(&self, device : SharedBlockDevice) -> FsResult<SharedRwFs> {
        log::info!("[fs::ext4-rs] mount_rw begin");
        let mut fs = Ext4RsFs::new();
        ReadWriteFs::mount_rw(&mut fs, device)?;
        Ok(Arc::new(Mutex::new(LocalRwFs::new(Box::new(fs)))))
    }
}

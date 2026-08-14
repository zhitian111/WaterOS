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
use api_v0::*;
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
/// Linux 路径解析允许的最大符号链接跳转次数。
const MAX_SYMLINK_DEPTH : usize = 40;

#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[fs/ext4-rs] self_test begin");
    assert_eq!(EXT4_SUPER_MAGIC, 0xEF53);
    assert_eq!(SUPERBLOCK_OFFSET, 1024);
    assert!(MAX_SYMLINK_DEPTH > 0);
    log::info!("[fs/ext4-rs] self_test complete");
}
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
        Errno::ENOTEMPTY => FsError::NotEmpty,
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

/// 读取符号链接 inode 的目标。
///
/// ext4 会把不超过 60 字节的“快速符号链接”直接放在 inode 的 `i_block` 字段中，
/// 此时不能走普通文件的 extent 读取路径，否则会把链接文本误当成 extent header。
fn read_symlink_inode(fs : &Ext4, inode : u32) -> FsResult<Vec<u8>> {
    let inode_ref = fs.get_inode_ref(inode);
    if inode_ref.inode.file_type() != InodeFileType::S_IFLNK {
        return Err(FsError::NotAFile);
    }
    let size = usize::try_from(inode_ref.inode.size()).map_err(|_| FsError::Io)?;
    if size <= 60 && inode_ref.inode.blocks_count() == 0 {
        let words = inode_ref.inode.block();
        let mut target = Vec::with_capacity(size);
        for word in words {
            target.extend_from_slice(&word.to_le_bytes());
        }
        target.truncate(size);
        return Ok(target);
    }
    let mut target = vec![0u8; size];
    if size > 0 {
        let n = fs.read_at(inode, 0, &mut target).map_err(map_ext4_rs)?;
        if n != size {
            return Err(FsError::Io);
        }
    }
    Ok(target)
}

fn push_path_components(out : &mut Vec<String>, path : &str) -> FsResult<()> {
    for part in path.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            let _ = out.pop();
            continue;
        }
        if part.len() > 255 {
            return Err(FsError::InvalidPath);
        }
        out.push(String::from(part));
    }
    Ok(())
}

fn components_to_absolute_path(parts : &[String]) -> String {
    if parts.is_empty() {
        return String::from("/");
    }
    let mut path = String::new();
    for part in parts {
        path.push('/');
        path.push_str(part.as_str());
    }
    path
}

/// 按路径逐级查找 inode。中间路径中的符号链接始终跟随；`follow_final` 决定是否
/// 跟随末级符号链接。这样 `metadata/read_symlink` 仍能观察链接本身，而普通文件读取
/// 和 ELF 加载可以取得链接指向的真实文件。
fn lookup_inode_inner(fs : &Ext4,
                      path : &str,
                      follow_final : bool,
                      depth : usize)
                      -> FsResult<u32> {
    if depth >= MAX_SYMLINK_DEPTH {
        return Err(FsError::InvalidPath);
    }
    let p = path.trim_end_matches('/');
    if p.is_empty() || p == "/" {
        return Ok(ROOT_INODE);
    }

    let mut inode = ROOT_INODE;
    let mut resolved_parts : Vec<String> = Vec::new();
    let mut parts = p.split('/')
                     .filter(|part| !part.is_empty())
                     .peekable();
    while let Some(part) = parts.next() {
        if part == "." || part == ".." || part.len() > 255 {
            return Err(FsError::InvalidPath);
        }
        let attr = fs.fuse_lookup(inode as u64, part)
                     .map_err(map_ext4_rs)?;
        let has_remaining = parts.peek().is_some();
        let child = u32::try_from(attr.ino).map_err(|_| FsError::Io)?;
        if attr.kind == InodeFileType::S_IFLNK && (has_remaining || follow_final) {
            let target = read_symlink_inode(fs, child)?;
            let target = core::str::from_utf8(target.as_slice()).map_err(|_| FsError::NotUtf8)?;
            let mut next_parts = if target.starts_with('/') {
                Vec::new()
            } else {
                resolved_parts.clone()
            };
            push_path_components(&mut next_parts, target)?;
            for remaining in parts {
                push_path_components(&mut next_parts, remaining)?;
            }
            let next_path = components_to_absolute_path(next_parts.as_slice());
            return lookup_inode_inner(fs, next_path.as_str(), follow_final, depth + 1);
        }
        if has_remaining && attr.kind != InodeFileType::S_IFDIR {
            return Err(FsError::NotAFile);
        }
        inode = child;
        resolved_parts.push(String::from(part));
    }
    Ok(inode)
}

fn lookup_inode(fs : &Ext4, path : &str) -> FsResult<u32> {
    lookup_inode_inner(fs, path, false, 0)
}

fn lookup_inode_follow(fs : &Ext4, path : &str) -> FsResult<u32> {
    lookup_inode_inner(fs, path, true, 0)
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
    let parent = lookup_inode_follow(fs, parent_path)?;
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
    let parent = lookup_inode_follow(fs, parent_path)?;
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


#[path = "../operations.rs"]
mod operations;
pub use operations::{Ext4RsImpl, IMPL};
#[cfg(feature = "self_test")]
pub use operations::self_test;

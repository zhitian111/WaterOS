//! 读写路径（基于 `ext4plus`，beta；写路径无完整 journal，仅用于 bring-up 与小文件测试）。
//!
//! I/O 边界：块读写适配器将驱动错误装箱为 `ext4plus` 期望的 `Error` trait object；按块读改写见本模块中的 `block_write_bytes`。

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use api_v0::{FsDirEntry, FsError, FsMetadata, FsNodeType, FsResult, ReadWriteFs};
use driver_block_api_v0::BLOCK_SIZE;
use ext4plus::file::File;
use ext4plus::Metadata;
use core::error::Error;
use core::time::Duration;
use driver_block_api_v0::{DriverError, Lba, SharedBlockDevice};
use ext4plus::dir::Dir;
use ext4plus::error::Ext4Error;
use ext4plus::file::{truncate, write_at};
use ext4plus::inode::{InodeCreationOptions, InodeFlags, InodeMode};
use ext4plus::path::Path;
use ext4plus::{DirEntryName, Ext4, Ext4Read, Ext4Write, FileType, FollowSymlinks};
use spin::Mutex;

static EXT4_SMALL_READ_CACHE: Mutex<SmallReadCache> = Mutex::new(SmallReadCache::new());

struct SmallReadCache {
    valid: bool,
    dev_id: usize,
    block: u64,
    data: [u8; BLOCK_SIZE],
}

impl SmallReadCache {
    const fn new() -> Self {
        Self {
            valid: false,
            dev_id: 0,
            block: 0,
            data: [0; BLOCK_SIZE],
        }
    }
}

fn shared_block_dev_id(dev: &SharedBlockDevice) -> usize {
    alloc::sync::Arc::as_ptr(dev) as *const () as usize
}

fn read_with_small_cache(dev: &SharedBlockDevice,
                         start_byte: u64,
                         dst: &mut [u8])
                         -> Result<bool, DriverError> {
    if dst.is_empty() {
        return Ok(true);
    }
    let start = usize::try_from(start_byte).map_err(|_| DriverError::InvalidParam)?;
    let end = start
        .checked_add(dst.len())
        .ok_or(DriverError::InvalidParam)?;
    if dst.len() > 64 || start / BLOCK_SIZE != (end - 1) / BLOCK_SIZE {
        let mut guard = dev.lock();
        guard.read_bytes(start_byte, dst)?;
        return Ok(false);
    }

    let block = (start / BLOCK_SIZE) as u64;
    let offset = start % BLOCK_SIZE;
    let dev_id = shared_block_dev_id(dev);
    {
        let cache = EXT4_SMALL_READ_CACHE.lock();
        if cache.valid && cache.dev_id == dev_id && cache.block == block {
            dst.copy_from_slice(&cache.data[offset..offset + dst.len()]);
            return Ok(true);
        }
    }

    let mut block_buf = [0u8; BLOCK_SIZE];
    {
        let mut guard = dev.lock();
        guard.read_blocks(Lba(block), &mut block_buf)?;
    }
    let mut cache = EXT4_SMALL_READ_CACHE.lock();
    cache.valid = true;
    cache.dev_id = dev_id;
    cache.block = block;
    cache.data.copy_from_slice(&block_buf);
    dst.copy_from_slice(&cache.data[offset..offset + dst.len()]);
    Ok(false)
}

fn invalidate_small_read_cache(dev: &SharedBlockDevice, start_byte: u64, len: usize) {
    if len == 0 {
        return;
    }
    let Ok(start) = usize::try_from(start_byte) else {
        EXT4_SMALL_READ_CACHE.lock().valid = false;
        return;
    };
    let Some(end) = start.checked_add(len) else {
        EXT4_SMALL_READ_CACHE.lock().valid = false;
        return;
    };
    let start_block = start / BLOCK_SIZE;
    let end_block = (end - 1) / BLOCK_SIZE;
    let dev_id = shared_block_dev_id(dev);
    let mut cache = EXT4_SMALL_READ_CACHE.lock();
    if cache.valid
        && cache.dev_id == dev_id
        && cache.block >= start_block as u64
        && cache.block <= end_block as u64
    {
        cache.valid = false;
    }
}

// 共享块设备句柄上的按字节读/写：同一 `SharedBlockDevice` 分别作为 reader 与 writer 传入 `load_with_writer`。
struct BlockDevRw {
    device: SharedBlockDevice,
}

impl Ext4Read for BlockDevRw {
    fn read(
        &self,
        start_byte: u64,
        dst: &mut [u8],
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        read_with_small_cache(&self.device, start_byte, dst)
            .map(|_| ())
            .map_err(driver_boxed)
    }
}

impl Ext4Write for BlockDevRw {
    fn write(
        &self,
        start_byte: u64,
        src: &[u8],
    ) -> Result<(), Box<dyn Error + Send + Sync + 'static>> {
        block_write_bytes(&self.device, start_byte, src).map_err(driver_boxed)
    }
}

fn driver_boxed(e: DriverError) -> Box<dyn Error + Send + Sync + 'static> { Box::new(DriverErr(e)) }

#[derive(Debug)]
struct DriverErr(DriverError);

impl core::fmt::Display for DriverErr {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl Error for DriverErr {}

// 按字节写入块设备：完整覆盖块直接写，非块对齐头尾才读-改-写。
fn block_write_bytes(
    dev: &SharedBlockDevice,
    start_byte: u64,
    src: &[u8],
) -> Result<(), DriverError> {
    if src.is_empty() {
        return Ok(());
    }
    let mut guard = dev.lock();
    let bdev: &mut dyn driver_block_api_v0::BlockDevice = &mut **guard;
    let bs = bdev.block_size();
    if bs == 0 {
        return Err(DriverError::InvalidParam);
    }
    let start = usize::try_from(start_byte).map_err(|_| DriverError::InvalidParam)?;
    let end = start
        .checked_add(src.len())
        .ok_or(DriverError::InvalidParam)?;

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
        bdev.write_blocks(Lba(block as u64), &src[pos..pos + full_bytes])?;
        pos += full_bytes;
    }

    if pos < src.len() {
        let abs = start + pos;
        debug_assert_eq!(abs % bs, 0);
        debug_assert!(end > abs);
        write_partial_block(bdev, abs / bs, 0, &src[pos..])?;
    }
    invalidate_small_read_cache(dev, start_byte, src.len());
    Ok(())
}

fn write_partial_block(
    bdev: &mut dyn driver_block_api_v0::BlockDevice,
    block: usize,
    offset: usize,
    data: &[u8],
) -> Result<(), DriverError> {
    let bs = bdev.block_size();
    if bs == 0 || offset >= bs || data.is_empty() || offset + data.len() > bs {
        return Err(DriverError::InvalidParam);
    }
    let mut block_buf = alloc::vec::Vec::new();
    block_buf.resize(bs, 0);
    bdev.read_blocks(Lba(block as u64), &mut block_buf)?;
    block_buf[offset..offset + data.len()].copy_from_slice(data);
    bdev.write_blocks(Lba(block as u64), &block_buf)
}

/// 将 `ext4plus` 错误映射为公共 [`FsError`]。
pub(crate) fn map_ext4_plus(err: Ext4Error) -> FsError {
    match err {
        Ext4Error::NotFound => FsError::NotFound,
        Ext4Error::NotAbsolute | Ext4Error::MalformedPath | Ext4Error::PathTooLong => {
            FsError::InvalidPath
        }
        Ext4Error::IsADirectory | Ext4Error::IsASpecialFile | Ext4Error::NotADirectory => {
            FsError::NotAFile
        }
        Ext4Error::NotUtf8 => FsError::NotUtf8,
        Ext4Error::Io(_) => FsError::Io,
        Ext4Error::Incompatible(_) => FsError::Unsupported,
        Ext4Error::Corrupt(_) => FsError::Corrupt,
        Ext4Error::Readonly => FsError::Unsupported,
        Ext4Error::AlreadyExists => FsError::Exists,
        _ => FsError::Unsupported,
    }
}

/// 读写 ext4 句柄。挂载成功后内部持有 `ext4plus::Ext4` 与可选 writer。
pub struct Ext4FsRw {
    fs: Option<Ext4>,
}

impl Ext4FsRw {
    /// 构造未挂载 RW 句柄；成功 [`ReadWriteFs::mount_rw`] 前其他方法返回 [`FsError::NotMounted`]。
    pub const fn new() -> Self { Self { fs: None } }

    fn fs(&self) -> FsResult<&Ext4> { self.fs.as_ref().ok_or(FsError::NotMounted) }
}

impl ReadWriteFs for Ext4FsRw {
    fn mount_rw(&mut self, device: SharedBlockDevice) -> FsResult<()> {
        let reader = Box::new(BlockDevRw { device: device.clone() });
        let writer = Box::new(BlockDevRw { device });
        let fs = Ext4::load_with_writer(reader, Some(writer)).map_err(map_ext4_plus)?;
        self.fs = Some(fs);
        Ok(())
    }

    fn is_mounted(&self) -> bool { self.fs.is_some() }

    fn write_regular_file_at_root(&mut self, name: &str, data: &[u8]) -> FsResult<()> {
        if name.is_empty() || name.contains('/') {
            return Err(FsError::InvalidPath);
        }
        let mut path = alloc::string::String::from("/");
        path.push_str(name);
        self.write_regular_file(path.as_str(), data)
    }

    fn write_regular_file(&mut self, path: &str, data: &[u8]) -> FsResult<()> {
        let (parent, name) = split_parent_and_name(path)?;
        let fs = self.fs()?;
        let name = DirEntryName::try_from(name).map_err(|_| FsError::InvalidPath)?;
        let parent_path = Path::try_from(parent).map_err(|_| FsError::InvalidPath)?;
        let parent_inode = fs
            .path_to_inode(parent_path, FollowSymlinks::All)
            .map_err(map_ext4_plus)?;
        if parent_inode.file_type() != FileType::Directory {
            return Err(FsError::NotFound);
        }
        let mut parent_dir = Dir::open_inode(fs, parent_inode).map_err(map_ext4_plus)?;

        match parent_dir.get_entry(name) {
            Ok(mut inode) => {
                if inode.file_type() != FileType::Regular {
                    return Err(FsError::NotAFile);
                }
                truncate(fs, &mut inode, 0).map_err(map_ext4_plus)?;
                if !data.is_empty() {
                    write_at(fs, &mut inode, data, 0).map_err(map_ext4_plus)?;
                }
                return Ok(());
            }
            Err(Ext4Error::NotFound) => {}
            Err(err) => return Err(map_ext4_plus(err)),
        }

        let mut inode = fs
            .create_inode(InodeCreationOptions {
                file_type: FileType::Regular,
                mode: InodeMode::S_IFREG
                    | InodeMode::S_IRUSR
                    | InodeMode::S_IWUSR
                    | InodeMode::S_IRGRP
                    | InodeMode::S_IROTH,
                uid: 0,
                gid: 0,
                time: Duration::from_secs(0),
                flags: InodeFlags::empty(),
            })
            .map_err(map_ext4_plus)?;

        if !data.is_empty() {
            write_at(fs, &mut inode, data, 0).map_err(map_ext4_plus)?;
        }
        parent_dir.link(name, &mut inode).map_err(map_ext4_plus)?;
        Ok(())
    }

    fn write_range(&mut self, path: &str, offset: u64, data: &[u8]) -> FsResult<usize> {
        if data.is_empty() {
            return Ok(0);
        }
        let fs = self.fs()?;
        let pathv = match Path::try_from(path) {
            Ok(pathv) => pathv,
            Err(_) => return Err(FsError::InvalidPath),
        };
        let mut inode = match fs.path_to_inode(pathv, FollowSymlinks::All) {
            Ok(inode) => inode,
            Err(err) => return Err(map_ext4_plus(err)),
        };
        if inode.file_type() != FileType::Regular {
            return Err(FsError::NotAFile);
        }
        let mut done = 0usize;
        while done < data.len() {
            let n = write_at(fs, &mut inode, &data[done..], offset + done as u64)
                .map_err(map_ext4_plus)?;
            if n == 0 {
                return Err(FsError::Io);
            }
            done += n;
        }
        Ok(done)
    }

    fn truncate(&mut self, path: &str, len: u64) -> FsResult<()> {
        let fs = self.fs()?;
        let pathv = Path::try_from(path).map_err(|_| FsError::InvalidPath)?;
        let mut inode = fs
            .path_to_inode(pathv, FollowSymlinks::All)
            .map_err(map_ext4_plus)?;
        if inode.file_type() != FileType::Regular {
            return Err(FsError::NotAFile);
        }
        truncate(fs, &mut inode, len).map_err(map_ext4_plus)
    }

    fn mkdir(&mut self, path: &str, mode: u32) -> FsResult<()> {
        let (parent, name) = split_parent_and_name(path)?;
        let fs = self.fs()?;
        let name = DirEntryName::try_from(name).map_err(|_| FsError::InvalidPath)?;
        let parent_path = Path::try_from(parent).map_err(|_| FsError::InvalidPath)?;
        let parent_inode = fs
            .path_to_inode(parent_path, FollowSymlinks::All)
            .map_err(map_ext4_plus)?;
        if parent_inode.file_type() != FileType::Directory {
            return Err(FsError::NotAFile);
        }
        let mut parent_dir = Dir::open_inode(fs, parent_inode).map_err(map_ext4_plus)?;

        if parent_dir.get_entry(name).is_ok() {
            return Err(FsError::Exists);
        }

        let inode_mode = linux_mkdir_mode_to_inode_mode(mode);
        let mut inode = fs
            .create_inode(InodeCreationOptions {
                file_type: FileType::Directory,
                mode: inode_mode,
                uid: 0,
                gid: 0,
                time: Duration::from_secs(0),
                flags: InodeFlags::empty(),
            })
            .map_err(map_ext4_plus)?;

        parent_dir.link(name, &mut inode).map_err(map_ext4_plus)?;
        Ok(())
    }

    fn chmod(&mut self, path: &str, mode: u32) -> FsResult<()> {
        let fs = self.fs()?;
        let pathv = Path::try_from(path).map_err(|_| FsError::InvalidPath)?;
        let mut inode = fs
            .path_to_inode(pathv, FollowSymlinks::All)
            .map_err(map_ext4_plus)?;
        let current = inode.mode();
        let new_mode = inode_type_bits(current) | linux_mode_perm_to_inode_mode(mode);
        inode.set_mode(new_mode).map_err(map_ext4_plus)?;
        inode.write(fs).map_err(map_ext4_plus)?;
        Ok(())
    }

    fn chown(&mut self, path: &str, uid: Option<u32>, gid: Option<u32>) -> FsResult<()> {
        if uid.is_none() && gid.is_none() {
            return self.metadata(path).map(|_| ());
        }
        let fs = self.fs()?;
        let pathv = Path::try_from(path).map_err(|_| FsError::InvalidPath)?;
        let mut inode = fs
            .path_to_inode(pathv, FollowSymlinks::All)
            .map_err(map_ext4_plus)?;
        if let Some(uid) = uid {
            inode.set_uid(uid);
        }
        if let Some(gid) = gid {
            inode.set_gid(gid);
        }
        inode.write(fs).map_err(map_ext4_plus)?;
        Ok(())
    }

    fn unlink(&mut self, path: &str) -> FsResult<()> {
        let (parent, name) = split_parent_and_name(path)?;
        let fs = self.fs()?;
        let parent_path = Path::try_from(parent).map_err(|_| FsError::InvalidPath)?;
        let parent_inode = fs
            .path_to_inode(parent_path, FollowSymlinks::All)
            .map_err(map_ext4_plus)?;
        if parent_inode.file_type() != FileType::Directory {
            return Err(FsError::NotFound);
        }
        let mut parent_dir = Dir::open_inode(fs, parent_inode).map_err(map_ext4_plus)?;
        let name = DirEntryName::try_from(name).map_err(|_| FsError::InvalidPath)?;
        let target = parent_dir.get_entry(name).map_err(map_ext4_plus)?;
        if target.file_type() == FileType::Directory {
            return Err(FsError::NotAFile);
        }
        if target.file_type() != FileType::Regular {
            return Err(FsError::Unsupported);
        }
        parent_dir.unlink(name, target).map_err(map_ext4_plus)?;
        Ok(())
    }

    fn rename(&mut self, old_path: &str, new_path: &str) -> FsResult<()> {
        let (old_parent, old_name) = split_parent_and_name(old_path)?;
        let (new_parent, new_name) = split_parent_and_name(new_path)?;
        if old_parent != new_parent {
            return Err(FsError::Unsupported);
        }
        let fs = self.fs()?;
        let parent_path = Path::try_from(old_parent).map_err(|_| FsError::InvalidPath)?;
        let parent_inode = fs
            .path_to_inode(parent_path, FollowSymlinks::All)
            .map_err(map_ext4_plus)?;
        if parent_inode.file_type() != FileType::Directory {
            return Err(FsError::NotFound);
        }
        let mut parent_dir = Dir::open_inode(fs, parent_inode).map_err(map_ext4_plus)?;
        let old_name = DirEntryName::try_from(old_name).map_err(|_| FsError::InvalidPath)?;
        let new_name = DirEntryName::try_from(new_name).map_err(|_| FsError::InvalidPath)?;
        let mut inode = parent_dir.get_entry(old_name).map_err(map_ext4_plus)?;
        if parent_dir.get_entry(new_name).is_ok() {
            return Err(FsError::Exists);
        }
        parent_dir
            .link(new_name, &mut inode)
            .map_err(map_ext4_plus)?;
        parent_dir.unlink(old_name, inode).map_err(map_ext4_plus)?;
        Ok(())
    }

    fn hardlink(&mut self, existing_path: &str, new_path: &str) -> FsResult<()> {
        let fs = self.fs()?;
        let existing_pathv = Path::try_from(existing_path).map_err(|_| FsError::InvalidPath)?;
        let mut inode = fs
            .path_to_inode(existing_pathv, FollowSymlinks::All)
            .map_err(map_ext4_plus)?;
        if inode.file_type() == FileType::Directory {
            return Err(FsError::NotAFile);
        }
        if inode.file_type() != FileType::Regular {
            return Err(FsError::Unsupported);
        }

        let (new_parent, new_name) = split_parent_and_name(new_path)?;
        let parent_path = Path::try_from(new_parent).map_err(|_| FsError::InvalidPath)?;
        let parent_inode = fs
            .path_to_inode(parent_path, FollowSymlinks::All)
            .map_err(map_ext4_plus)?;
        if parent_inode.file_type() != FileType::Directory {
            return Err(FsError::NotFound);
        }
        let mut parent_dir = Dir::open_inode(fs, parent_inode).map_err(map_ext4_plus)?;
        let new_name = DirEntryName::try_from(new_name).map_err(|_| FsError::InvalidPath)?;

        if parent_dir.get_entry(new_name).is_ok() {
            return Err(FsError::Exists);
        }

        parent_dir.link(new_name, &mut inode).map_err(map_ext4_plus)?;
        Ok(())
    }

    fn rmdir(&mut self, path: &str) -> FsResult<()> {
        let (parent, name) = split_parent_and_name(path)?;
        let fs = self.fs()?;
        let parent_path = Path::try_from(parent).map_err(|_| FsError::InvalidPath)?;
        let parent_inode = fs
            .path_to_inode(parent_path, FollowSymlinks::All)
            .map_err(map_ext4_plus)?;
        if parent_inode.file_type() != FileType::Directory {
            return Err(FsError::NotFound);
        }
        let mut parent_dir = Dir::open_inode(fs, parent_inode).map_err(map_ext4_plus)?;
        let name = DirEntryName::try_from(name).map_err(|_| FsError::InvalidPath)?;
        let target = parent_dir.get_entry(name).map_err(map_ext4_plus)?;
        if target.file_type() != FileType::Directory {
            return Err(FsError::NotAFile);
        }
        let pathv = Path::try_from(path).map_err(|_| FsError::InvalidPath)?;
        let mut rd = fs.read_dir(pathv).map_err(map_ext4_plus)?;
        while let Some(item) = rd.next() {
            let ent = item.map_err(map_ext4_plus)?;
            let n = ent.file_name();
            if n.as_ref() != b"." && n.as_ref() != b".." {
                return Err(FsError::Exists);
            }
        }
        parent_dir.unlink(name, target).map_err(map_ext4_plus)?;
        Ok(())
    }

    fn exists(&self, path: &str) -> FsResult<bool> {
        let fs = self.fs()?;
        let pathv = Path::try_from(path).map_err(|_| FsError::InvalidPath)?;
        fs.exists(pathv).map_err(map_ext4_plus)
    }

    fn metadata(&self, path: &str) -> FsResult<FsMetadata> {
        let fs = self.fs()?;
        let pathv = Path::try_from(path).map_err(|_| FsError::InvalidPath)?;
        let inode = fs
            .path_to_inode(pathv, FollowSymlinks::All)
            .map_err(map_ext4_plus)?;
        let meta = fs.metadata(pathv).map_err(map_ext4_plus)?;
        Ok(map_rw_metadata(&meta, u64::from(inode.index.get())))
    }

    fn read(&self, path: &str) -> FsResult<Vec<u8>> {
        let fs = self.fs()?;
        let pathv = Path::try_from(path).map_err(|_| FsError::InvalidPath)?;
        let meta = fs.metadata(pathv).map_err(map_ext4_plus)?;
        if !meta.file_type().is_regular_file() {
            return Err(FsError::NotAFile);
        }
        let file_size = usize::try_from(meta.len()).map_err(|_| FsError::Io)?;
        let inode = fs
            .path_to_inode(pathv, FollowSymlinks::All)
            .map_err(map_ext4_plus)?;
        let mut file = File::open_inode(fs, inode).map_err(map_ext4_plus)?;
        let mut out = vec![0u8; file_size];
        let mut filled = 0usize;
        while filled < file_size {
            let room = file_size - filled;
            let chunk = room.min(BLOCK_SIZE);
            let n = file
                .read_bytes(&mut out[filled..filled + chunk])
                .map_err(map_ext4_plus)?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        if filled != file_size {
            return Err(FsError::Io);
        }
        Ok(out)
    }

    fn read_range(&self, path: &str, offset: u64, buf: &mut [u8]) -> FsResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let fs = self.fs()?;
        let pathv = Path::try_from(path).map_err(|_| FsError::InvalidPath)?;
        let meta = fs.metadata(pathv).map_err(map_ext4_plus)?;
        if !meta.file_type().is_regular_file() {
            return Err(FsError::NotAFile);
        }
        let file_size = meta.len();
        if offset >= file_size {
            return Ok(0);
        }
        let inode = fs
            .path_to_inode(pathv, FollowSymlinks::All)
            .map_err(map_ext4_plus)?;
        let mut file = File::open_inode(fs, inode).map_err(map_ext4_plus)?;
        file.seek_to(offset).map_err(map_ext4_plus)?;
        let max_read = usize::try_from(file_size - offset).map_err(|_| FsError::Io)?;
        let to_read = buf.len().min(max_read);
        let mut filled = 0usize;
        while filled < to_read {
            let room = to_read - filled;
            let chunk = room.min(BLOCK_SIZE);
            let n = file
                .read_bytes(&mut buf[filled..filled + chunk])
                .map_err(map_ext4_plus)?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        Ok(filled)
    }

    fn read_dir(&self, path: &str) -> FsResult<Vec<FsDirEntry>> {
        let fs = self.fs()?;
        let pathv = Path::try_from(path).map_err(|_| FsError::InvalidPath)?;
        let mut rd = fs.read_dir(pathv).map_err(map_ext4_plus)?;
        let mut out = Vec::new();
        while let Some(item) = rd.next() {
            let ent = item.map_err(map_ext4_plus)?;
            let name = ent.file_name();
            if name.as_ref() == b"." || name.as_ref() == b".." {
                continue;
            }
            let name_str = core::str::from_utf8(name.as_ref()).map_err(|_| FsError::NotUtf8)?;
            let ft = ent.file_type().map_err(map_ext4_plus)?;
            let node_type = if ft.is_dir() {
                FsNodeType::Directory
            } else if ft.is_symlink() {
                FsNodeType::Symlink
            } else if ft.is_regular_file() {
                FsNodeType::File
            } else {
                FsNodeType::Special
            };
            out.push(FsDirEntry {
                name: String::from(name_str),
                node_type,
            });
        }
        Ok(out)
    }
}

fn map_rw_metadata(meta: &Metadata, inode: u64) -> FsMetadata {
    let node_type = if meta.is_dir() {
        FsNodeType::Directory
    } else if meta.is_symlink() {
        FsNodeType::Symlink
    } else if meta.file_type().is_regular_file() {
        FsNodeType::File
    } else {
        FsNodeType::Special
    };
    FsMetadata {
        node_type,
        size: meta.len(),
        mode: meta.mode(),
        inode,
        nlink: u32::from(meta.links_count),
    }
}

/// 将 Linux `mkdir(2)` 的 `mode`（权限位，可含 `S_IFDIR`）映射为 ext4 `InodeMode`。
fn linux_mkdir_mode_to_inode_mode(mode: u32) -> InodeMode {
    InodeMode::S_IFDIR | linux_mode_perm_to_inode_mode(mode)
}

/// 保留 ext4 inode 的文件类型位。
fn inode_type_bits(mode: InodeMode) -> InodeMode {
    mode & (InodeMode::S_IFIFO
        | InodeMode::S_IFCHR
        | InodeMode::S_IFDIR
        | InodeMode::S_IFBLK
        | InodeMode::S_IFREG
        | InodeMode::S_IFLNK
        | InodeMode::S_IFSOCK)
}

/// 将 Linux `chmod(2)` / `fchmodat(2)` 的权限位（`mode & 0o7777`）映射为 ext4 `InodeMode`。
fn linux_mode_perm_to_inode_mode(mode: u32) -> InodeMode {
    let perm = mode & 0o7777;
    let mut m = InodeMode::empty();
    if perm & 0o4000 != 0 {
        m |= InodeMode::S_ISUID;
    }
    if perm & 0o2000 != 0 {
        m |= InodeMode::S_ISGID;
    }
    if perm & 0o1000 != 0 {
        m |= InodeMode::S_ISVTX;
    }
    if perm & 0o400 != 0 {
        m |= InodeMode::S_IRUSR;
    }
    if perm & 0o200 != 0 {
        m |= InodeMode::S_IWUSR;
    }
    if perm & 0o100 != 0 {
        m |= InodeMode::S_IXUSR;
    }
    if perm & 0o040 != 0 {
        m |= InodeMode::S_IRGRP;
    }
    if perm & 0o020 != 0 {
        m |= InodeMode::S_IWGRP;
    }
    if perm & 0o010 != 0 {
        m |= InodeMode::S_IXGRP;
    }
    if perm & 0o004 != 0 {
        m |= InodeMode::S_IROTH;
    }
    if perm & 0o002 != 0 {
        m |= InodeMode::S_IWOTH;
    }
    if perm & 0o001 != 0 {
        m |= InodeMode::S_IXOTH;
    }
    m
}

/// 将绝对路径拆成 `(父目录路径, 最终分量名)`；`path` 须为指向文件的绝对路径。
fn split_parent_and_name(path: &str) -> FsResult<(&str, &str)> {
    let p = path.trim_end_matches('/');
    if p.is_empty() || p == "/" {
        return Err(FsError::InvalidPath);
    }
    let (parent, name) = p.rsplit_once('/').ok_or(FsError::InvalidPath)?;
    let parent = if parent.is_empty() { "/" } else { parent };
    if name.is_empty() || name.contains('/') {
        return Err(FsError::InvalidPath);
    }
    Ok((parent, name))
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use alloc::sync::Arc;
    use alloc::vec;
    use driver_block_api_v0::{BlockDevice, BLOCK_SIZE};
    use spin::Mutex;

    struct CountingBlockDevice {
        bytes: Arc<Mutex<Vec<u8>>>,
        reads: Arc<Mutex<usize>>,
        writes: Arc<Mutex<usize>>,
    }

    impl CountingBlockDevice {
        fn shared(
            blocks: usize,
        ) -> (
            SharedBlockDevice,
            Arc<Mutex<Vec<u8>>>,
            Arc<Mutex<usize>>,
            Arc<Mutex<usize>>,
        ) {
            let bytes = Arc::new(Mutex::new(vec![0xaau8; blocks * BLOCK_SIZE]));
            let reads = Arc::new(Mutex::new(0));
            let writes = Arc::new(Mutex::new(0));
            let dev = Self {
                bytes: bytes.clone(),
                reads: reads.clone(),
                writes: writes.clone(),
            };
            (
                Arc::new(Mutex::new(Box::new(dev) as Box<dyn BlockDevice>)),
                bytes,
                reads,
                writes,
            )
        }
    }

    impl BlockDevice for CountingBlockDevice {
        fn read_blocks(&mut self, start_block: Lba, buf: &mut [u8]) -> Result<(), DriverError> {
            if buf.len() % BLOCK_SIZE != 0 {
                return Err(DriverError::InvalidParam);
            }
            *self.reads.lock() += 1;
            let start = usize::try_from(start_block.0)
                .map_err(|_| DriverError::InvalidParam)?
                .checked_mul(BLOCK_SIZE)
                .ok_or(DriverError::InvalidParam)?;
            let end = start.checked_add(buf.len()).ok_or(DriverError::InvalidParam)?;
            let bytes = self.bytes.lock();
            let src = bytes.get(start..end).ok_or(DriverError::InvalidParam)?;
            buf.copy_from_slice(src);
            Ok(())
        }

        fn write_blocks(&mut self, start_block: Lba, buf: &[u8]) -> Result<(), DriverError> {
            if buf.len() % BLOCK_SIZE != 0 {
                return Err(DriverError::InvalidParam);
            }
            *self.writes.lock() += 1;
            let start = usize::try_from(start_block.0)
                .map_err(|_| DriverError::InvalidParam)?
                .checked_mul(BLOCK_SIZE)
                .ok_or(DriverError::InvalidParam)?;
            let end = start.checked_add(buf.len()).ok_or(DriverError::InvalidParam)?;
            let mut bytes = self.bytes.lock();
            let dst = bytes.get_mut(start..end).ok_or(DriverError::InvalidParam)?;
            dst.copy_from_slice(buf);
            Ok(())
        }
    }

    #[test]
    fn aligned_full_blocks_write_without_read_modify_write() {
        let (dev, bytes, reads, writes) = CountingBlockDevice::shared(4);
        let src = vec![0x5au8; BLOCK_SIZE * 2];

        block_write_bytes(&dev, BLOCK_SIZE as u64, &src).unwrap();

        assert_eq!(*reads.lock(), 0);
        assert_eq!(*writes.lock(), 1);
        let bytes = bytes.lock();
        assert_eq!(&bytes[..BLOCK_SIZE], &[0xaau8; BLOCK_SIZE]);
        assert_eq!(&bytes[BLOCK_SIZE..BLOCK_SIZE * 3], src.as_slice());
        assert_eq!(&bytes[BLOCK_SIZE * 3..BLOCK_SIZE * 4], &[0xaau8; BLOCK_SIZE]);
    }

    #[test]
    fn unaligned_head_and_tail_preserve_uncovered_bytes() {
        let (dev, bytes, reads, writes) = CountingBlockDevice::shared(4);
        let len = (BLOCK_SIZE - 3) + BLOCK_SIZE + 7;
        let src: Vec<u8> = (0..len).map(|i| (i % 251) as u8).collect();

        block_write_bytes(&dev, 3, &src).unwrap();

        assert_eq!(*reads.lock(), 2);
        assert_eq!(*writes.lock(), 3);
        let bytes = bytes.lock();
        assert_eq!(&bytes[..3], &[0xaau8; 3]);
        assert_eq!(&bytes[3..3 + len], src.as_slice());
        assert_eq!(&bytes[3 + len..BLOCK_SIZE * 3], &[0xaau8; BLOCK_SIZE - 7]);
    }
}

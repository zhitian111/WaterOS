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
        self.device
            .lock()
            .read_bytes(start_byte, dst)
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

// 按块读-改-写：依赖驱动提供块对齐 I/O；非块对齐尾段通过整块缓冲完成。
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
    let mut pos = 0usize;
    while pos < src.len() {
        let abs = usize::try_from(start_byte)
            .map_err(|_| DriverError::InvalidParam)?
            .checked_add(pos)
            .ok_or(DriverError::InvalidParam)?;
        let start_block = abs / bs;
        let o = abs % bs;
        let room = bs.checked_sub(o).ok_or(DriverError::InvalidParam)?;
        let take = room.min(src.len() - pos);
        let mut block_buf = alloc::vec::Vec::new();
        block_buf.resize(bs, 0);
        bdev.read_blocks(Lba(start_block as u64), &mut block_buf)?;
        block_buf[o..o + take].copy_from_slice(&src[pos..pos + take]);
        bdev.write_blocks(Lba(start_block as u64), &block_buf)?;
        pos += take;
    }
    Ok(())
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
        let pathv = Path::try_from(path).map_err(|_| FsError::InvalidPath)?;
        let mut inode = fs
            .path_to_inode(pathv, FollowSymlinks::All)
            .map_err(map_ext4_plus)?;
        if inode.file_type() != FileType::Regular {
            return Err(FsError::NotAFile);
        }
        write_at(fs, &mut inode, data, offset).map_err(map_ext4_plus)
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
    let perm = mode & 0o777;
    let mut m = InodeMode::S_IFDIR;
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

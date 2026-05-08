//! 读写路径（基于 `ext4plus`，beta；写路径无完整 journal，仅用于 bring-up 与小文件测试）。
//!
//! I/O 边界：块读写适配器将驱动错误装箱为 `ext4plus` 期望的 `Error` trait object；按块读改写见本模块中的 `block_write_bytes`。

use alloc::boxed::Box;
use api_v0::{FsError, FsResult, ReadWriteFs};
use core::error::Error;
use core::time::Duration;
use driver_block_api_v0::{DriverError, Lba, SharedBlockDevice};
use ext4plus::dir::Dir;
use ext4plus::error::Ext4Error;
use ext4plus::file::write_at;
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
        Ext4Error::AlreadyExists => FsError::InvalidPath,
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
        let fs = self.fs()?;
        let name = DirEntryName::try_from(name).map_err(|_| FsError::InvalidPath)?;

        let root_inode = fs.read_root_inode().map_err(map_ext4_plus)?;
        let mut root = Dir::open_inode(fs, root_inode).map_err(map_ext4_plus)?;

        // 根目录下同名普通文件则先 unlink，保证「写」语义近似 create/replace。
        if let Ok(old) = root.get_entry(name) {
            root.unlink(name, old).map_err(map_ext4_plus)?;
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

        // 数据写入 inode 后再 link 进根目录，顺序依赖 ext4plus 对未链接 inode 的约定。
        write_at(fs, &mut inode, data, 0).map_err(map_ext4_plus)?;
        root.link(name, &mut inode).map_err(map_ext4_plus)?;
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

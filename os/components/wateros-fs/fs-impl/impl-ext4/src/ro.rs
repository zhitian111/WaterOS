//! 只读路径（基于 `ext4-view`）：实现 [`api_v0::ReadOnlyFs`] 与启动期目录树打印。

use alloc::boxed::Box;
use alloc::vec::Vec;
use api_v0::{FsError, FsMetadata, FsNodeType, FsResult, ReadOnlyFs};
use driver_block_api_v0::SharedBlockDevice;
use ext4_view::{Ext4, Ext4Error, Ext4Read, Metadata, Path};

// 将驱动的按字节读适配为 ext4-view 的 Ext4Read；错误映射在 map_ext4_error。
struct BlockDeviceReader {
    device: SharedBlockDevice,
}

impl Ext4Read for BlockDeviceReader {
    fn read(
        &mut self,
        start_byte: u64,
        dst: &mut [u8],
    ) -> Result<(), Box<dyn core::error::Error + Send + Sync + 'static>> {
        self.device
            .lock()
            .read_bytes(start_byte, dst)
            .map_err(|err| Box::new(BlockIoError(err)) as _)
    }
}

#[derive(Debug)]
struct BlockIoError(driver_block_api_v0::DriverError);

impl core::fmt::Display for BlockIoError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "block io error: {:?}", self.0)
    }
}

impl core::error::Error for BlockIoError {}

/// 只读 ext4 句柄。挂载成功后内部持有 `ext4-view::Ext4`。
pub struct Ext4Fs {
    fs: Option<Ext4>,
}

impl Ext4Fs {
    /// 构造未挂载句柄；成功 [`ReadOnlyFs::mount`] 前其他方法返回 [`FsError::NotMounted`]。
    pub const fn new() -> Self { Self { fs: None } }

    fn fs(&self) -> FsResult<&Ext4> { self.fs.as_ref().ok_or(FsError::NotMounted) }
}

impl ReadOnlyFs for Ext4Fs {
    fn mount(&mut self, device: SharedBlockDevice) -> FsResult<()> {
        let fs = Ext4::load(Box::new(BlockDeviceReader { device })).map_err(map_ext4_error)?;
        self.fs = Some(fs);
        Ok(())
    }

    fn is_mounted(&self) -> bool { self.fs.is_some() }

    fn exists(&self, path: &str) -> FsResult<bool> {
        self.fs()?.exists(path).map_err(map_ext4_error)
    }

    fn metadata(&self, path: &str) -> FsResult<FsMetadata> {
        let metadata = self.fs()?.metadata(path).map_err(map_ext4_error)?;
        Ok(map_metadata(&metadata))
    }

    fn read(&self, path: &str) -> FsResult<Vec<u8>> {
        self.fs()?.read(path).map_err(map_ext4_error)
    }

    fn boot_dump_all_paths(&self) {
        let Some(ext4) = self.fs.as_ref() else { return };
        walk_ext4_tree(ext4, Path::ROOT);
    }
}

// 启动期调试：深度优先打印；遇读目录错误则静默剪枝，避免 init 失败。
fn walk_ext4_tree(ext4: &Ext4, dir: Path<'_>) {
    let Ok(rd) = ext4.read_dir(dir) else { return };
    for item in rd {
        let Ok(ent) = item else { continue };
        let name = ent.file_name();
        if name.as_ref() == b"." || name.as_ref() == b".." {
            continue;
        }
        let p = ent.path();
        logging::info!("[fs::boot-tree] {}", p.display());
        if let Ok(ft) = ent.file_type() {
            if ft.is_dir() {
                walk_ext4_tree(ext4, p.as_path());
            }
        }
    }
}

fn map_metadata(meta: &Metadata) -> FsMetadata {
    let node_type = if meta.is_dir() {
        FsNodeType::Directory
    } else if meta.is_symlink() {
        FsNodeType::Symlink
    } else if meta.file_type().is_regular_file() {
        FsNodeType::File
    } else {
        FsNodeType::Special
    };

    FsMetadata { node_type, size: meta.len(), mode: meta.mode() }
}

/// 将 `ext4-view` 错误映射为公共 [`FsError`]（供 ro 与测试复用）。
pub(crate) fn map_ext4_error(err: Ext4Error) -> FsError {
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
        _ => FsError::Unsupported,
    }
}

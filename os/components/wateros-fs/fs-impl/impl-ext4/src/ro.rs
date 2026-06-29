//! 只读路径（基于 `ext4plus`）：实现 [`api_v0::ReadOnlyFs`] 与启动期目录树打印。
//!
//! 大块文件读路径刻意按 `driver_block_api_v0::BLOCK_SIZE` 分片，规避部分 VirtIO 小扇区组合下的一次性整读问题（见本文件中 `ReadOnlyFs::read` 实现内注释）。
//! 本模块代码由AI完成

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use api_v0::{FsDirEntry, FsError, FsMetadata, FsNodeType, FsResult, ReadOnlyFs};
use driver_block_api_v0::{SharedBlockDevice, BLOCK_SIZE};
use ext4plus::path::Path;
use ext4plus::{Ext4, Ext4Read, FollowSymlinks, Metadata};

use crate::boot_inspect;
use crate::rw::map_ext4_plus;

// 只读块设备适配器；错误经 `BlockIoError` 装箱给 ext4plus。
// 本结构代码由AI完成
struct BlockDeviceReader {
    device: SharedBlockDevice,
}

impl Ext4Read for BlockDeviceReader {
// 本方法代码由AI完成
    fn read(
        &self,
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
// 本结构代码由AI完成
struct BlockIoError(driver_block_api_v0::DriverError);

impl core::fmt::Display for BlockIoError {
// 本方法代码由AI完成
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "block io error: {:?}", self.0)
    }
}

impl core::error::Error for BlockIoError {}

/// 只读 ext4 句柄。挂载成功后内部持有 `ext4plus::Ext4`。
// 本结构代码由AI完成
pub struct Ext4Fs {
    fs: Option<Ext4>,
}

impl Ext4Fs {
    /// 构造未挂载句柄；成功 [`ReadOnlyFs::mount`] 前其他方法返回 [`FsError::NotMounted`]。
    pub const fn new() -> Self { Self { fs: None } }

    fn fs(&self) -> FsResult<&Ext4> { self.fs.as_ref().ok_or(FsError::NotMounted) }
}

impl ReadOnlyFs for Ext4Fs {
// 本方法代码由AI完成
    fn mount(&mut self, device: SharedBlockDevice) -> FsResult<()> {
        let fs = Ext4::load(Box::new(BlockDeviceReader { device })).map_err(map_ext4_plus)?;
        self.fs = Some(fs);
        Ok(())
    }

    fn is_mounted(&self) -> bool { self.fs.is_some() }

// 本方法代码由AI完成
    fn exists(&self, path: &str) -> FsResult<bool> {
        let path = Path::try_from(path).map_err(|_| FsError::InvalidPath)?;
        self.fs()?.exists(path).map_err(map_ext4_plus)
    }

// 本方法代码由AI完成
    fn metadata(&self, path: &str) -> FsResult<FsMetadata> {
        let fs = self.fs()?;
        let path = Path::try_from(path).map_err(|_| FsError::InvalidPath)?;
        let inode = fs
            .path_to_inode(path, FollowSymlinks::All)
            .map_err(map_ext4_plus)?;
        let metadata = fs.metadata(path).map_err(map_ext4_plus)?;
        Ok(map_metadata(&metadata, u64::from(inode.index.get())))
    }

// 本方法代码由AI完成
    fn read_dir(&self, path: &str) -> FsResult<Vec<FsDirEntry>> {
        let fs = self.fs()?;
        let pathv = Path::try_from(path).map_err(|_| FsError::InvalidPath)?;
        let rd = fs.read_dir(pathv).map_err(map_ext4_plus)?;
        let mut out = Vec::new();
        for item in rd {
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

// 本方法代码由AI完成
    fn read(&self, path: &str) -> FsResult<Vec<u8>> {
        // 不用 `Ext4::read` 单次 `read_inode_file`：在大量目录遍历 + 大块整读时，
        // 曾与内核侧 VirtIO 512B 扇区路径组合出现首读 ELF 头损坏；按块设备逻辑块
        // 粒度循环 `File::read_bytes` 组装整文件更稳。
        let fs = self.fs()?;
        let pathv = Path::try_from(path).map_err(|_| FsError::InvalidPath)?;
        let meta = fs.metadata(pathv).map_err(map_ext4_plus)?;
        if !meta.file_type().is_regular_file() {
            return Err(FsError::NotAFile);
        }
        let file_size = usize::try_from(meta.len()).map_err(|_| FsError::Io)?;
        let mut file = fs.open(pathv).map_err(map_ext4_plus)?;
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

// 本方法代码由AI完成
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
        let mut file = fs.open(pathv).map_err(map_ext4_plus)?;
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

// 本方法代码由AI完成
    fn read_prefix(&self, path: &str, len: usize) -> FsResult<Vec<u8>> {
        let mut buf = vec![0u8; len];
        let n = self.read_range(path, 0, &mut buf)?;
        buf.truncate(n);
        Ok(buf)
    }

// 本方法代码由AI完成
    fn boot_dump_all_paths(&self) {
        let Some(ext4) = self.fs.as_ref() else { return };
        walk_ext4_tree(ext4, Path::ROOT);
    }
}

/// 启动期从 ext4 读普通文件，最多 `cap` 字节（与 [`ReadOnlyFs::read`] 相同分片策略）。
// 本方法代码由AI完成
fn read_regular_file_capped(ext4: &Ext4, path: Path<'_>, cap: usize) -> Option<Vec<u8>> {
    let meta = ext4.metadata(path).ok()?;
    if !meta.file_type().is_regular_file() {
        return None;
    }
    let total = usize::try_from(meta.len()).ok()?;
    let to_read = total.min(cap);
    let mut file = ext4.open(path).ok()?;
    let mut out = vec![0u8; to_read];
    let mut filled = 0usize;
    while filled < to_read {
        let room = to_read - filled;
        let chunk = room.min(BLOCK_SIZE);
        let n = file
            .read_bytes(&mut out[filled..filled + chunk])
            .ok()?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    (filled == to_read).then_some(out)
}

// 启动期调试：深度优先打印路径；对 `.sh` 打印正文预览，对带执行位且为 ELF64 LE 的文件打印头信息。
// 本方法代码由AI完成
fn walk_ext4_tree(ext4: &Ext4, dir: Path<'_>) {
    let Ok(rd) = ext4.read_dir(dir) else { return };
    for item in rd {
        let Ok(ent) = item else { continue };
        let name = ent.file_name();
        if name.as_ref() == b"." || name.as_ref() == b".." {
            continue;
        }
        let p = ent.path();
        let path_display = format!("{}", p.display());
        logging::trace!("[fs::boot-tree] {}", path_display);
        if let Ok(ft) = ent.file_type() {
            if ft.is_dir() {
                walk_ext4_tree(ext4, p.as_path());
            } else if ft.is_regular_file() {
                let Ok(meta) = ext4.metadata(p.as_path()) else {
                    continue;
                };
                let mode = meta.mode();
                let name_b = name.as_ref();
                if boot_inspect::should_dump_sh_script(name_b) {
                    if let Some(bytes) =
                        read_regular_file_capped(ext4, p.as_path(), boot_inspect::MAX_SH_BOOT_BYTES)
                    {
                        boot_inspect::log_sh_file(path_display.as_str(), &bytes);
                    }
                }
                if let Some(prefix) = read_regular_file_capped(ext4, p.as_path(), 64) {
                    boot_inspect::log_elf_exec_if_applicable(path_display.as_str(), &prefix, mode);
                }
            }
        }
    }
}

// 本方法代码由AI完成
fn map_metadata(meta: &Metadata, inode: u64) -> FsMetadata {
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
        uid: meta.uid(),
        gid: meta.gid(),
    }
}

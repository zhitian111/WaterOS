#![no_std]
extern crate alloc;

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use api_v0::{install_root_fs, root_fs, FsError, FsMetadata, FsNodeType, FsResult, LocalFs, ReadOnlyFs, SharedFs};
use driver_block_api_v0::{block_device_count, first_block_device, SharedBlockDevice};
use ext4_view::{Ext4, Ext4Error, Ext4Read, Metadata};
use spin::Mutex;

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

pub struct Ext4ViewFs {
    fs: Option<Ext4>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ElfHeaderInfo {
    class: u8,
    data: u8,
    machine: u16,
    entry: u64,
}

impl Ext4ViewFs {
    pub const fn new() -> Self {
        Self { fs: None }
    }

    fn fs(&self) -> FsResult<&Ext4> {
        self.fs.as_ref().ok_or(FsError::NotMounted)
    }
}

impl ReadOnlyFs for Ext4ViewFs {
    fn mount(&mut self, device: SharedBlockDevice) -> FsResult<()> {
        let fs = Ext4::load(Box::new(BlockDeviceReader { device }))
            .map_err(map_ext4_error)?;
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

    FsMetadata {
        node_type,
        size: meta.len(),
        mode: meta.mode(),
    }
}

fn map_ext4_error(err: Ext4Error) -> FsError {
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

fn parse_elf_header(data: &[u8]) -> FsResult<ElfHeaderInfo> {
    if data.len() < 0x40 {
        return Err(FsError::Io);
    }
    if &data[0..4] != b"\x7fELF" {
        return Err(FsError::Corrupt);
    }

    let class = data[4];
    let data_encoding = data[5];
    let read_u16 = |offset: usize| -> FsResult<u16> {
        let bytes = data.get(offset..offset + 2).ok_or(FsError::Corrupt)?;
        Ok(match data_encoding {
            1 => u16::from_le_bytes([bytes[0], bytes[1]]),
            2 => u16::from_be_bytes([bytes[0], bytes[1]]),
            _ => return Err(FsError::Unsupported),
        })
    };
    let read_u64 = |offset: usize| -> FsResult<u64> {
        let bytes = data.get(offset..offset + 8).ok_or(FsError::Corrupt)?;
        Ok(match data_encoding {
            1 => u64::from_le_bytes(bytes.try_into().map_err(|_| FsError::Corrupt)?),
            2 => u64::from_be_bytes(bytes.try_into().map_err(|_| FsError::Corrupt)?),
            _ => return Err(FsError::Unsupported),
        })
    };

    let machine = read_u16(18)?;
    let entry = match class {
        2 => read_u64(24)?,
        _ => return Err(FsError::Unsupported),
    };

    Ok(ElfHeaderInfo {
        class,
        data: data_encoding,
        machine,
        entry,
    })
}

pub fn init() -> FsResult<()> {
    if root_fs().is_some() {
        return Ok(());
    }

    let device = first_block_device().ok_or(FsError::NotMounted)?;
    let mut fs = Ext4ViewFs::new();
    fs.mount(device)?;
    let shared: SharedFs = Arc::new(Mutex::new(LocalFs::new(Box::new(fs))));
    install_root_fs(shared);
    log::info!("[fs::ext4-view] mounted root fs from first block device");
    Ok(())
}

pub fn test() -> FsResult<()> {
    if block_device_count() == 0 {
        return Err(FsError::NotMounted);
    }

    init()?;
    let fs = root_fs().ok_or(FsError::NotMounted)?;
    let fs = fs.lock();

    for path in ["/src/bin/000_hello_world.rs", "/elf/000_hello_world"] {
        match fs.metadata(path) {
            Ok(meta) => {
                log::info!(
                    "[fs::ext4-view] path={} type={:?} size={} mode={:#o}",
                    path,
                    meta.node_type,
                    meta.size,
                    meta.mode
                );
            }
            Err(err) => {
                log::warn!("[fs::ext4-view] metadata failed for {}: {:?}", path, err);
            }
        }
    }

    if let Ok(text) = fs.read_prefix("/src/bin/000_hello_world.rs", 96) {
        let preview = String::from_utf8_lossy(&text);
        log::info!("[fs::ext4-view] text prefix: {}", preview);
    }

    if let Ok(elf_head) = fs.read_prefix("/elf/000_hello_world", 64) {
        log::info!("[fs::ext4-view] elf first16={:02x?}", &elf_head[..elf_head.len().min(16)]);
        match parse_elf_header(&elf_head) {
            Ok(info) => log::info!(
                "[fs::ext4-view] elf header class={} data={} machine={:#x} entry={:#x}",
                info.class,
                info.data,
                info.machine,
                info.entry
            ),
            Err(err) => log::warn!("[fs::ext4-view] parse elf header failed: {:?}", err),
        }
    }

    Ok(())
}

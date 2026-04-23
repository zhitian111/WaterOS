#![no_std]
extern crate alloc;

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use api_v0::{FsError, FsMetadata, FsNodeType, FsResult, LocalFs, ReadOnlyFs, SharedFs};
use driver_block_api_v0::SharedBlockDevice;
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

pub fn mount_by_block_path(path: &str) -> FsResult<SharedFs> {
    logging::info!("[fs::ext4-view] mount begin, device_path={}", path);
    let device = devfs::active_impl::lookup_block_device(path)?;
    let mut fs = Ext4ViewFs::new();
    fs.mount(device)?;
    let shared: SharedFs = Arc::new(Mutex::new(LocalFs::new(Box::new(fs))));
    Ok(shared)
}

pub fn test_with(fs: SharedFs) -> FsResult<()> {
    let fs = fs.lock();

    const TEXT_PATH: &str = "/src/bin/000_hello_world.rs";
    const ELF_PATH: &str = "/elf/000_hello_world";

    for path in [TEXT_PATH, ELF_PATH] {
        match fs.metadata(path) {
            Ok(meta) => {
                logging::info!(
                    "[fs::ext4-view][test] metadata OK path={} type={:?} size={} mode={:#o}",
                    path,
                    meta.node_type,
                    meta.size,
                    meta.mode
                );
            }
            Err(err) => {
                logging::warn!(
                    "[fs::ext4-view][test] metadata FAIL path={} err={:?}",
                    path,
                    err
                );
            }
        }
    }

    match fs.read_prefix(TEXT_PATH, 96) {
        Ok(text) => {
            let preview = String::from_utf8_lossy(&text);
            let hex_preview_len = text.len().min(32);
            logging::info!(
                "[fs::ext4-view][test] read OK path={} bytes={} hex_prefix[0..{}]={:02x?} utf8_preview={:?}",
                TEXT_PATH,
                text.len(),
                hex_preview_len,
                &text[..hex_preview_len],
                preview.as_ref()
            );
        }
        Err(err) => {
            logging::warn!(
                "[fs::ext4-view][test] read FAIL path={} err={:?}",
                TEXT_PATH,
                err
            );
        }
    }

    match fs.read_prefix(ELF_PATH, 64) {
        Ok(elf_head) => {
            let n = elf_head.len().min(16);
            logging::info!(
                "[fs::ext4-view][test] read OK path={} bytes={} first16={:02x?}",
                ELF_PATH,
                elf_head.len(),
                &elf_head[..n]
            );
            match parse_elf_header(&elf_head) {
                Ok(info) => {
                    logging::info!(
                        "[fs::ext4-view][test] elf header OK class={} data={} machine={:#x} entry={:#x}",
                        info.class,
                        info.data,
                        info.machine,
                        info.entry
                    );
                }
                Err(err) => {
                    logging::warn!(
                        "[fs::ext4-view][test] elf header PARSE FAIL err={:?} raw64={:02x?}",
                        err,
                        elf_head.as_slice()
                    );
                }
            }
        }
        Err(err) => {
            logging::warn!(
                "[fs::ext4-view][test] read FAIL path={} err={:?}",
                ELF_PATH,
                err
            );
        }
    }

    Ok(())
}

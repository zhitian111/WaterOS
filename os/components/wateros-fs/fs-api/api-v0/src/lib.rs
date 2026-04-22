#![no_std]
extern crate alloc;

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use core::ops::{Deref, DerefMut};
use driver_block_api_v0::SharedBlockDevice;
use spin::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    NotMounted,
    NotFound,
    NotAFile,
    InvalidPath,
    NotUtf8,
    Unsupported,
    Driver,
    Corrupt,
    Io,
}

pub type FsResult<T> = core::result::Result<T, FsError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsNodeType {
    File,
    Directory,
    Symlink,
    Special,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsMetadata {
    pub node_type: FsNodeType,
    pub size: u64,
    pub mode: u16,
}

pub trait ReadOnlyFs {
    fn mount(&mut self, device: SharedBlockDevice) -> FsResult<()>;
    fn is_mounted(&self) -> bool;
    fn exists(&self, path: &str) -> FsResult<bool>;
    fn metadata(&self, path: &str) -> FsResult<FsMetadata>;
    fn read(&self, path: &str) -> FsResult<Vec<u8>>;

    fn read_prefix(&self, path: &str, len: usize) -> FsResult<Vec<u8>> {
        let mut data = self.read(path)?;
        if data.len() > len {
            data.truncate(len);
        }
        Ok(data)
    }

    fn read_to_string(&self, path: &str) -> FsResult<String> {
        let data = self.read(path)?;
        String::from_utf8(data).map_err(|_| FsError::NotUtf8)
    }
}

pub struct LocalFs(Box<dyn ReadOnlyFs>);

impl LocalFs {
    pub fn new(inner: Box<dyn ReadOnlyFs>) -> Self { Self(inner) }
}

impl Deref for LocalFs {
    type Target = dyn ReadOnlyFs;

    fn deref(&self) -> &Self::Target { &*self.0 }
}

impl DerefMut for LocalFs {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut *self.0 }
}

// 当前阶段按单核串行访问使用文件系统对象。
unsafe impl Send for LocalFs {}

pub type SharedFs = Arc<Mutex<LocalFs>>;

static ROOT_FS: Mutex<Option<SharedFs>> = Mutex::new(None);

pub fn install_root_fs(fs: SharedFs) {
    *ROOT_FS.lock() = Some(fs);
}

pub fn root_fs() -> Option<SharedFs> {
    ROOT_FS.lock().as_ref().cloned()
}

pub fn test() {
    log::trace!("[fs-api] test begin");
    let fs = SampleFs;
    let text = fs.read_to_string("/hello.txt").expect("sample text");
    assert_eq!(text, "hello");
    let meta = fs.metadata("/hello.txt").expect("sample metadata");
    assert_eq!(meta.size, 5);
    log::trace!("[fs-api] test end");
}

struct SampleFs;

impl ReadOnlyFs for SampleFs {
    fn mount(&mut self, _device: SharedBlockDevice) -> FsResult<()> { Ok(()) }

    fn is_mounted(&self) -> bool { true }

    fn exists(&self, path: &str) -> FsResult<bool> { Ok(path == "/hello.txt") }

    fn metadata(&self, path: &str) -> FsResult<FsMetadata> {
        if path == "/hello.txt" {
            Ok(FsMetadata {
                node_type: FsNodeType::File,
                size: 5,
                mode: 0o644,
            })
        } else {
            Err(FsError::NotFound)
        }
    }

    fn read(&self, path: &str) -> FsResult<Vec<u8>> {
        if path == "/hello.txt" {
            Ok(b"hello".to_vec())
        } else {
            Err(FsError::NotFound)
        }
    }
}

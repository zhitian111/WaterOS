//! 已打开目录句柄：供 `openat(dirfd, …)` 与 `getdents64` 使用。

extern crate alloc;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;

use api_v0::{
    SingleRootReadView, VfsDirEntry, VfsError, VfsIoHandle, VfsMetadata, VfsNodeType, VfsResult,
};

use crate::FsBridge;

/// 根卷上已打开的目录（缓存 `read_dir` 结果供 `getdents64` 续读）。
#[derive(Clone)]
pub struct DirectoryHandle {
    path: String,
    meta: VfsMetadata,
    dirents: Option<Vec<VfsDirEntry>>,
    next_index: usize,
}

impl DirectoryHandle {
    pub(crate) fn open(bridge: &FsBridge, path: String) -> VfsResult<Box<dyn VfsIoHandle>> {
        if !bridge.exists(path.as_str())? {
            return Err(VfsError::NotFound);
        }
        let meta = bridge.metadata(path.as_str())?;
        if meta.node_type != VfsNodeType::Directory {
            return Err(VfsError::NotAFile);
        }
        Ok(Box::new(Self {
            path,
            meta,
            dirents: None,
            next_index: 0,
        }))
    }

    fn load_dirents(&mut self, bridge: &FsBridge) -> VfsResult<()> {
        if self.dirents.is_some() {
            return Ok(());
        }
        self.dirents = Some(bridge.read_dir(self.path.as_str())?);
        self.next_index = 0;
        Ok(())
    }
}

const DT_REG: u8 = 8;
const DT_DIR: u8 = 4;
const DT_LNK: u8 = 10;
const HEADER_SIZE: usize = 19;

fn dirent64_reclen(name_len: usize) -> usize {
    let with_name = HEADER_SIZE + name_len + 1;
    (with_name + 7) & !7
}

pub(crate) fn node_type_to_dt(t: VfsNodeType) -> u8 {
    match t {
        VfsNodeType::File => DT_REG,
        VfsNodeType::Directory => DT_DIR,
        VfsNodeType::Symlink => DT_LNK,
        VfsNodeType::Special => 0,
    }
}

pub(crate) fn dirent64_encode_slice(
    buf: &mut [u8],
    ino: u64,
    next_off: i64,
    name: &str,
    d_type: u8,
) -> Option<usize> {
    encode_one(buf, ino, next_off, name, d_type)
}

pub(crate) fn encode_one(buf: &mut [u8], ino: u64, next_off: i64, name: &str, d_type: u8) -> Option<usize> {
    let reclen = dirent64_reclen(name.len());
    if buf.len() < reclen {
        return None;
    }
    buf[0..8].copy_from_slice(&ino.to_le_bytes());
    buf[8..16].copy_from_slice(&next_off.to_le_bytes());
    buf[16..18].copy_from_slice(&(reclen as u16).to_le_bytes());
    buf[18] = d_type;
    let name_start = HEADER_SIZE;
    let nb = name.as_bytes();
    buf[name_start..name_start + nb.len()].copy_from_slice(nb);
    buf[name_start + nb.len()] = 0;
    let pad = name_start + nb.len() + 1;
    for b in &mut buf[pad..reclen] {
        *b = 0;
    }
    Some(reclen)
}

impl VfsIoHandle for DirectoryHandle {
    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(self.meta.clone())
    }

    fn directory_path(&self) -> Option<&str> {
        Some(self.path.as_str())
    }

    fn backing_path(&self) -> Option<&str> {
        Some(self.path.as_str())
    }

    fn read(&mut self, _buf: &mut [u8]) -> VfsResult<usize> {
        Err(VfsError::NotAFile)
    }

    fn write(&mut self, _buf: &[u8]) -> VfsResult<usize> {
        Err(VfsError::NotAFile)
    }

    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(self.clone()))
    }

    fn fill_getdents64(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        let bridge = FsBridge;
        self.load_dirents(&bridge)?;
        let entries = self.dirents.as_ref().expect("load_dirents");
        let mut out = 0usize;
        let mut off = 0usize;
        while self.next_index < entries.len() {
            let ent = &entries[self.next_index];
            let next_off = (self.next_index + 1) as i64;
            let d_type = node_type_to_dt(ent.node_type);
            let slice = &mut buf[off..];
            let Some(reclen) = encode_one(slice, 1, next_off, ent.name.as_str(), d_type) else {
                break;
            };
            off += reclen;
            out += reclen;
            self.next_index += 1;
        }
        Ok(out)
    }
}

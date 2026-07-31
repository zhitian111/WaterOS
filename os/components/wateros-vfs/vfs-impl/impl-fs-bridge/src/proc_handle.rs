//! procfs 伪挂载打开句柄。
//! 本模块代码由AI完成

extern crate alloc;

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};

use api_v0::{
    VfsDirEntry, VfsError, VfsIoHandle, VfsMetadata, VfsNodeType, VfsOpenDescriptionState,
    VfsOpenFlags, VfsResult, VfsSeekWhence,
};
use fs::procfs::api::ProcFsView;

use crate::dir_handle::{encode_one, node_type_to_dt};
use crate::mount_table::MountIdentity;
use crate::{map_fs_err, map_fs_node, map_meta};

// 本方法代码由AI完成
fn proc_view() -> &'static impl ProcFsView {
    fs::procfs::active_impl::view()
}

/// procfs 目录句柄。
#[derive(Clone)]
// 本结构代码由AI完成
pub struct ProcDirectoryHandle {
    /// 挂载内相对路径（不含 `/proc` 前缀）。
    rel: String,
    /// 用户可见绝对路径。
    abs: String,
    meta: VfsMetadata,
    dirents: Option<Vec<VfsDirEntry>>,
    description: Arc<VfsOpenDescriptionState>,
}

/// procfs 普通文件句柄（内容按需生成并缓存）。
#[derive(Clone)]
// 本结构代码由AI完成
pub struct ProcFileHandle {
    #[allow(dead_code)]
    rel: String,
    meta: VfsMetadata,
    data: Vec<u8>,
    description: Arc<VfsOpenDescriptionState>,
}

// 本方法代码由AI完成
pub fn open_proc(
    rel: String,
    abs: String,
    flags: VfsOpenFlags,
    identity: MountIdentity,
) -> VfsResult<Box<dyn VfsIoHandle>> {
    if flags.contains(VfsOpenFlags::WRITE) {
        return Err(VfsError::ReadOnlyFs);
    }
    let view = proc_view();
    if !view.exists(rel.as_str()).map_err(map_fs_err)? {
        return Err(VfsError::NotFound);
    }
    let fs_meta = view.metadata(rel.as_str()).map_err(map_fs_err)?;
    let meta = map_meta(fs_meta, identity);
    if meta.node_type == VfsNodeType::Directory {
        if flags.contains(VfsOpenFlags::DIRECTORY) || !flags.contains(VfsOpenFlags::WRITE) {
            return Ok(Box::new(ProcDirectoryHandle {
                rel,
                abs,
                meta,
                dirents: None,
                description: Arc::new(VfsOpenDescriptionState::new(0, 0)),
            }));
        }
        return Err(VfsError::NotAFile);
    }
    if flags.contains(VfsOpenFlags::DIRECTORY) {
        return Err(VfsError::NotAFile);
    }
    let data = view.read(rel.as_str()).map_err(map_fs_err)?;
    Ok(Box::new(ProcFileHandle {
        rel,
        meta,
        data,
        description: Arc::new(VfsOpenDescriptionState::new(0, 0)),
    }))
}

impl ProcDirectoryHandle {
// 本方法代码由AI完成
    fn load_dirents(&mut self) -> VfsResult<()> {
        if self.dirents.is_some() {
            return Ok(());
        }
        let entries = proc_view()
            .read_dir(self.rel.as_str())
            .map_err(map_fs_err)?;
        self.dirents = Some(
            entries
                .into_iter()
                .map(|e| VfsDirEntry {
                    name: e.name,
                    node_type: map_fs_node(e.node_type),
                })
                .collect(),
        );
        Ok(())
    }
}

impl VfsIoHandle for ProcDirectoryHandle {
    fn validate_read_access(&self) -> VfsResult<()> { Err(VfsError::NotAFile) }

    fn open_accmode(&self) -> u32 { 0 }

// 本方法代码由AI完成
    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(self.meta.clone())
    }

// 本方法代码由AI完成
    fn directory_path(&self) -> Option<&str> {
        Some(self.abs.as_str())
    }

// 本方法代码由AI完成
    fn read(&mut self, _buf: &mut [u8]) -> VfsResult<usize> {
        Err(VfsError::NotAFile)
    }

// 本方法代码由AI完成
    fn write(&mut self, _buf: &[u8]) -> VfsResult<usize> {
        Err(VfsError::ReadOnlyFs)
    }

// 本方法代码由AI完成
    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(self.clone()))
    }

// 本方法代码由AI完成
    fn fill_getdents64(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        self.load_dirents()?;
        let entries = self.dirents.as_ref().expect("load_dirents");
        let mut out = 0usize;
        let mut off = 0usize;
        let mut next_index = usize::try_from(self.description.offset()).map_err(|_| VfsError::Io)?;
        while next_index < entries.len() {
            let ent = &entries[next_index];
            let next_off = (next_index + 1) as i64;
            let d_type = node_type_to_dt(ent.node_type);
            let slice = &mut buf[off..];
            let Some(reclen) = encode_one(slice, 1, next_off, ent.name.as_str(), d_type) else {
                break;
            };
            off += reclen;
            out += reclen;
            next_index += 1;
            self.description.set_offset(next_index as u64);
        }
        Ok(out)
    }
}

impl VfsIoHandle for ProcFileHandle {
    fn open_accmode(&self) -> u32 { 0 }

// 本方法代码由AI完成
    fn metadata(&self) -> VfsResult<VfsMetadata> {
        Ok(self.meta.clone())
    }

// 本方法代码由AI完成
    fn read(&mut self, buf: &mut [u8]) -> VfsResult<usize> {
        let start = usize::try_from(self.description.offset()).map_err(|_| VfsError::Io)?;
        if start >= self.data.len() {
            return Ok(0);
        }
        let n = core::cmp::min(buf.len(), self.data.len() - start);
        buf[..n].copy_from_slice(&self.data[start..start + n]);
        self.description.advance_offset(n as u64)?;
        Ok(n)
    }

// 本方法代码由AI完成
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        let start = offset as usize;
        if start >= self.data.len() {
            return Ok(0);
        }
        let n = core::cmp::min(buf.len(), self.data.len() - start);
        buf[..n].copy_from_slice(&self.data[start..start + n]);
        Ok(n)
    }

// 本方法代码由AI完成
    fn write(&mut self, _buf: &[u8]) -> VfsResult<usize> {
        Err(VfsError::ReadOnlyFs)
    }

// 本方法代码由AI完成
    fn seek(&mut self, offset: i64, whence: VfsSeekWhence) -> VfsResult<u64> {
        let new_off = match whence {
            VfsSeekWhence::Set => offset.max(0) as u64,
            VfsSeekWhence::Cur => return self.description.add_signed_offset(offset),
            VfsSeekWhence::End => self
                .data
                .len()
                .saturating_add_signed(offset as isize) as u64,
        };
        self.description.set_offset(new_off);
        Ok(new_off)
    }

// 本方法代码由AI完成
    fn duplicate(&self) -> VfsResult<Box<dyn VfsIoHandle>> {
        Ok(Box::new(self.clone()))
    }
}

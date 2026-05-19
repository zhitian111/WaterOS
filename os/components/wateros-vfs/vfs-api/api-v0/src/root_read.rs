//! 单根只读路径级访问。

extern crate alloc;

use alloc::{string::String, vec::Vec};

use crate::error::{VfsError, VfsResult};
use crate::meta::{VfsDirEntry, VfsMetadata};
/// 单根只读视图：在已挂载根卷上提供路径级访问。
pub trait SingleRootReadView {
    /// `path` 须为绝对路径；未挂载返回 [`VfsError::NotMounted`]。
    fn exists(&self, path: &str) -> VfsResult<bool>;

    fn metadata(&self, path: &str) -> VfsResult<VfsMetadata>;

    fn read(&self, path: &str) -> VfsResult<Vec<u8>>;

    fn read_prefix(&self, path: &str, len: usize) -> VfsResult<Vec<u8>> {
        let mut data = self.read(path)?;
        if data.len() > len {
            data.truncate(len);
        }
        Ok(data)
    }

    fn read_to_string(&self, path: &str) -> VfsResult<String> {
        let data = self.read(path)?;
        String::from_utf8(data).map_err(|_| VfsError::NotUtf8)
    }

    fn read_dir(&self, path: &str) -> VfsResult<Vec<VfsDirEntry>> {
        let _ = path;
        Err(VfsError::Unsupported)
    }

    fn boot_dump_all_paths(&self) {}
}

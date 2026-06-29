//! 单根只读路径级访问。

extern crate alloc;

use alloc::{string::String, vec::Vec};

use crate::error::{VfsError, VfsResult};
use crate::meta::{VfsDirEntry, VfsMetadata};

/// 单根只读视图：在已挂载根卷上提供路径级访问。
pub trait SingleRootReadView {
    /// `path` 须为绝对路径；未挂载返回 [`VfsError::NotMounted`]。
    fn exists(&self, path: &str) -> VfsResult<bool>;

    /// 查询路径节点元数据；不存在时返回 [`VfsError::NotFound`]。
    fn metadata(&self, path: &str) -> VfsResult<VfsMetadata>;

    /// 读取普通文件全文；目录或特殊节点由后端返回相应错误。
    fn read(&self, path: &str) -> VfsResult<Vec<u8>>;

    fn read_range(&self, path: &str, offset: u64, buf: &mut [u8]) -> VfsResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let data = self.read(path)?;
        let start = usize::try_from(offset).map_err(|_| VfsError::Io)?;
        if start >= data.len() {
            return Ok(0);
        }
        let n = buf.len().min(data.len() - start);
        buf[..n].copy_from_slice(&data[start..start + n]);
        Ok(n)
    }

    fn read_prefix(&self, path: &str, len: usize) -> VfsResult<Vec<u8>> {
        let mut data = Vec::new();
        data.resize(len, 0);
        let n = self.read_range(path, 0, &mut data)?;
        data.truncate(n);
        Ok(data)
    }

    fn read_to_string(&self, path: &str) -> VfsResult<String> {
        let data = self.read(path)?;
        String::from_utf8(data).map_err(|_| VfsError::NotUtf8)
    }

    /// 枚举目录项；默认不支持（伪 FS 或 bring-up 后端可覆盖）。
    fn read_dir(&self, path: &str) -> VfsResult<Vec<VfsDirEntry>> {
        let _ = path;
        Err(VfsError::Unsupported)
    }

    /// 启动期可选：遍历并打印根卷全部路径（调试用，默认空实现）。
    fn boot_dump_all_paths(&self) {}
}

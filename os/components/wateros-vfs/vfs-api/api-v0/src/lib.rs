//! VFS **公共 API v0**：单根只读访问、可选根级可写会话、错误与元数据类型，以及绝对路径规范化。
//!
//! 实现方（`vfs-impl-*`）提供 [`SingleRootReadView`] / [`RootRwSession`] 的具体后端；本 crate **不** 依赖块设备或具体 FS 格式。

#![no_std]
extern crate alloc;

mod path;

pub use path::{normalize_absolute_path, NormalizedPath};

use alloc::{string::String, vec::Vec};

/// VFS 路径与 I/O 操作的统一错误面；与具体 FS 后端映射关系由 `vfs-impl-*` 保证语义对齐。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsError {
    /// 根卷或所需设备未就绪、未挂载。
    NotMounted,
    /// 规范化路径下无对应节点。
    NotFound,
    /// 操作要求普通文件，当前节点类型不符。
    NotAFile,
    /// 路径非法（如非绝对、空段约定违反、根文件名含 `/` 等，依调用点文档为准）。
    InvalidPath,
    /// 按 UTF-8 解释文件内容失败。
    NotUtf8,
    /// 当前构建或后端不支持该操作。
    Unsupported,
    /// 块设备或驱动层错误。
    Driver,
    /// 元数据或结构与 FS 约定不一致。
    Corrupt,
    /// 读写字节与预期不一致等泛 I/O 语义失败。
    Io,
}

/// [`VfsError`] 上的 [`core::result::Result`] 别名，供 trait 与 helper 统一签名。
pub type VfsResult<T> = core::result::Result<T, VfsError>;

/// 节点类型，供 [`VfsMetadata`] 与只读查询使用；与 POSIX `S_IF*` 细节无关，仅表达 VFS 抽象分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsNodeType {
    File,
    Directory,
    Symlink,
    Special,
}

/// 路径查询返回的轻量元数据；`mode` 含义与后端一致（如 Unix 风格位），本 crate 不解释具体编码。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VfsMetadata {
    /// 节点分类。
    pub node_type: VfsNodeType,
    /// 对文件类节点为逻辑长度；目录等由后端定义。
    pub size: u64,
    /// 权限或类型位，由实现透传。
    pub mode: u16,
}

/// 目录枚举单条结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VfsDirEntry {
    /// 目录项名字（非路径）。
    pub name: String,
    /// 节点类型。
    pub node_type: VfsNodeType,
}

/// 单根只读视图：由具体 impl（如 fs 桥接）在已挂载根卷上提供路径级访问。
pub trait SingleRootReadView {
    /// `path` 须为绝对路径；实现可先 [`normalize_absolute_path`] 再查卷。未挂载返回 [`VfsError::NotMounted`]。
    fn exists(&self, path: &str) -> VfsResult<bool>;

    /// 返回 `path` 对应节点的 [`VfsMetadata`]；不存在则 [`VfsError::NotFound`]。
    fn metadata(&self, path: &str) -> VfsResult<VfsMetadata>;

    /// 读入整个文件内容；非文件或过大由后端返回相应 [`VfsError`]。
    fn read(&self, path: &str) -> VfsResult<Vec<u8>>;

    /// 与 [`read`] 相同，但最多保留前 `len` 字节（用于启动期小文件探测）。
    fn read_prefix(&self, path: &str, len: usize) -> VfsResult<Vec<u8>> {
        let mut data = self.read(path)?;
        if data.len() > len {
            data.truncate(len);
        }
        Ok(data)
    }

    /// 将 [`read`] 结果按 UTF-8 解码为 [`String`]；非法 UTF-8 映射为 [`VfsError::NotUtf8`]。
    fn read_to_string(&self, path: &str) -> VfsResult<String> {
        let data = self.read(path)?;
        String::from_utf8(data).map_err(|_| VfsError::NotUtf8)
    }

    fn read_dir(&self, path: &str) -> VfsResult<Vec<VfsDirEntry>> {
        let _ = path;
        Err(VfsError::Unsupported)
    }

    /// 启动期调试：默认空操作；桥接可委托底层 `ReadOnlyFs::boot_dump_all_paths`。
    fn boot_dump_all_paths(&self) {}
}

/// 可写会话：与只读根句柄分离（例如独立 `SharedRwFs`），由 impl 管理生命周期。
pub trait RootRwSession {
    /// 在根目录下创建或覆盖名为 `name` 的普通文件；`name` 不得含 `/`（由桥接等实现校验）。
    fn write_regular_file_at_root(&mut self, name: &str, data: &[u8]) -> VfsResult<()>;

    /// 删除绝对路径指向的普通文件；默认 [`VfsError::Unsupported`]。
    fn unlink(&mut self, path: &str) -> VfsResult<()> {
        let _ = path;
        Err(VfsError::Unsupported)
    }
}

/// `api-v0` 内建单元测试：路径规范化与错误契约的固定样例。
pub fn test() {
    assert_eq!(
        normalize_absolute_path("/a/./b/../c").unwrap().as_str(),
        "/a/c"
    );
    assert_eq!(normalize_absolute_path("//").unwrap().as_str(), "/");
}

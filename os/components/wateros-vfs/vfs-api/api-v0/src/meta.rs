//! 节点元数据与目录项类型。

use alloc::string::String;

/// 节点类型；与 POSIX `S_IF*` 细节无关，仅表达 VFS 抽象分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VfsNodeType {
    File,
    Directory,
    Symlink,
    Special,
}

/// 路径查询返回的轻量元数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VfsMetadata {
    pub node_type: VfsNodeType,
    pub size: u64,
    pub mode: u16,
}

/// 目录枚举单条结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VfsDirEntry {
    pub name: String,
    pub node_type: VfsNodeType,
}

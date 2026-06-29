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
    /// Linux `st_dev` 的 major/minor 组成部分。
    pub device_major: u32,
    pub device_minor: u32,
    /// 文件系统内 inode 编号。
    pub inode: u64,
    /// 挂载实例编号，对应 `statx.stx_mnt_id`。
    pub mount_id: u64,
    /// 指向该 inode 的硬链接数量。
    pub nlink: u32,
    /// 属主 uid（Linux `st_uid`）。
    pub uid: u32,
    /// 属组 gid（Linux `st_gid`）。
    pub gid: u32,
}

/// 目录枚举单条结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VfsDirEntry {
    /// 目录项 basename（不含 `/`）。
    pub name: String,
    pub node_type: VfsNodeType,
}

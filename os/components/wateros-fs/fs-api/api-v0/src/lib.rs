#![no_std]

//! 文件系统 API（v0）：错误与能力枚举、只读/可写根卷 trait、[`FsImpl`] 聚合注册面及 [`SharedFs`] / [`SharedRwFs`] 共享句柄。
//!
//! 本 crate 为 `no_std`；共享句柄使用 `spin::Mutex`，调用方需保证与平台调度策略一致的访问边界。
extern crate alloc;

use alloc::{boxed::Box, string::String, sync::Arc, vec::Vec};
use core::ops::{Deref, DerefMut};
use driver_block_api_v0::SharedBlockDevice;
use spin::Mutex;

/// 文件系统操作错误；实现方将底层 I/O 与格式错误映射到此枚举。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    /// 根卷未挂载或句柄未就绪。
    NotMounted,
    /// 路径不存在。
    NotFound,
    /// 期望文件但目标为目录或特殊节点等。
    NotAFile,
    /// 路径非法、过长或不符合实现约束。
    InvalidPath,
    /// 目标路径已存在（如 `mkdir` 时目录项冲突）。
    Exists,
    /// 内容非合法 UTF-8（如 `read_to_string`）。
    NotUtf8,
    /// 操作或组合不被当前实现支持。
    Unsupported,
    /// 块设备驱动返回错误。
    Driver,
    /// 卷元数据或结构损坏。
    Corrupt,
    /// 通用 I/O 失败（非驱动分类错误）。
    Io,
    /// 无剩余空间；cgroup cpuset 无可用 cpus/mems 时拒绝 attach 也用此语义。
    NoSpace,
}

/// [`FsError`] 上的结果别名。
pub type FsResult<T> = core::result::Result<T, FsError>;

/// 文件系统类型标识。`Other` 用于子系统级别的虚拟 FS（如 devfs），便于注册表统一登记。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FsKind {
    /// ext2 族（探测或能力声明用）。
    Ext2,
    /// ext3 族。
    Ext3,
    /// ext4（当前 RO/RW 实现主要对应此 kind）。
    Ext4,
    /// 内核设备文件树（非块卷 FS）。
    DevFs,
    /// 其他具名子系统；字符串为稳定展示名。
    Other(&'static str),
}

/// 文件系统挂载访问模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FsAccessMode {
    /// 只读挂载。
    ReadOnly,
    /// 读写挂载。
    ReadWrite,
}

/// 单条能力声明：某 impl 支持的 (FsKind, FsAccessMode) 组合。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FsCapability {
    /// 文件系统种类。
    pub kind: FsKind,
    /// 访问模式。
    pub access: FsAccessMode,
}

impl FsCapability {
    /// 构造一条 `(kind, access)` 能力声明，供 impl 的 `supported()` 静态表使用。
    pub const fn new(kind: FsKind, access: FsAccessMode) -> Self { Self { kind, access } }
}

/// VFS/调试用的节点类型分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsNodeType {
    /// 普通文件。
    File,
    /// 目录。
    Directory,
    /// 符号链接。
    Symlink,
    /// 其他特殊 inode（设备节点等），具体语义依赖实现。
    Special,
}

/// 路径对应的元数据快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsMetadata {
    /// 节点类型。
    pub node_type: FsNodeType,
    /// 以字节为单位的大小（目录实现可能为 0 或近似值）。
    pub size: u64,
    /// Unix 风格 mode 位（实现相关）。
    pub mode: u16,
    /// 文件系统内部 inode 编号；同一文件的硬链接必须返回相同编号。
    pub inode: u64,
    /// 指向该 inode 的硬链接数量。
    pub nlink: u32,
    /// 属主 uid（Linux `st_uid`）。
    pub uid: u32,
    /// 属组 gid（Linux `st_gid`）。
    pub gid: u32,
}

/// 根卷文件 I/O 模式（与 `wateros-base-config::fs::FileIoMode` 对齐）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsIoMode {
    /// 同步区间读写。
    Direct,
    /// 异步区间读写（v1 未实现）。
    Async,
}

/// 目录枚举单条结果：仅含名字与类型（不含完整路径）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FsDirEntry {
    /// 目录项名字（非路径）。
    pub name: String,
    /// 节点类型。
    pub node_type: FsNodeType,
}

/// 只读根卷：挂载后对绝对路径提供存在性、元数据与整文件读取。
///
/// 路径契约：实现通常要求绝对路径并以 `/` 开头；具体规则见各 impl 文档。
pub trait ReadOnlyFs {
    /// 从块设备加载只读卷状态；重复调用语义由实现定义（建议幂等或返回错误）。
    fn mount(&mut self, device: SharedBlockDevice) -> FsResult<()>;
    /// 是否已完成挂载且可服务读请求。
    fn is_mounted(&self) -> bool;
    /// 路径是否存在（文件或目录均可能为 true，依实现）。
    fn exists(&self, path: &str) -> FsResult<bool>;
    /// 查询元数据；不存在返回 [`FsError::NotFound`]。
    fn metadata(&self, path: &str) -> FsResult<FsMetadata>;
    /// 读取整个文件内容到内存；大文件场景调用方需注意内存边界。
    fn read(&self, path: &str) -> FsResult<Vec<u8>>;

    /// 从 `offset` 起读取最多 `buf.len()` 字节到 `buf`；返回实际读取长度（EOF 为短读或 `0`）。
    fn read_range(&self, path: &str, offset: u64, buf: &mut [u8]) -> FsResult<usize> {
        let _ = (path, offset, buf);
        Err(FsError::Unsupported)
    }

    /// 读取文件前缀，最多 `len` 字节（短于文件则截断）。
    ///
    /// 默认回退为整文件 [`read`] 后截断；实现方应覆盖为 [`read_range`] 以避免大文件全量分配。
    fn read_prefix(&self, path: &str, len: usize) -> FsResult<Vec<u8>> {
        let mut data = self.read(path)?;
        if data.len() > len {
            data.truncate(len);
        }
        Ok(data)
    }

    /// 读取整个文件并校验为 UTF-8 字符串。
    fn read_to_string(&self, path: &str) -> FsResult<String> {
        let data = self.read(path)?;
        String::from_utf8(data).map_err(|_| FsError::NotUtf8)
    }

    /// 列出目录内容；`.` / `..` 由实现决定是否包含（ext4 实现通常跳过）。
    fn read_dir(&self, path: &str) -> FsResult<Vec<FsDirEntry>> {
        let _ = path;
        Err(FsError::Unsupported)
    }

    /// 读取符号链接目标（不含尾部的 `\0`）。
    fn read_symlink(&self, path: &str) -> FsResult<Vec<u8>> {
        let _ = path;
        Err(FsError::Unsupported)
    }

    /// 启动阶段调试：从根卷 `/` 起递归打印路径（实现方可覆盖；默认无操作）。
    fn boot_dump_all_paths(&self) {}
}

/// 可写根卷：与 [`ReadOnlyFs`] 分离，避免在 `dyn ReadOnlyFs` 上混入写语义；由 `ext4plus` 等实现承载，且实现类型须为 `Send`。
pub trait ReadWriteFs: Send {
    /// 以读写方式挂载；底层写能力依赖具体实现（如 journal 完整性）。
    fn mount_rw(&mut self, device: SharedBlockDevice) -> FsResult<()>;
    /// 是否已完成 RW 挂载。
    fn is_mounted(&self) -> bool;

    /// 在根目录下创建或替换名为 `name` 的普通文件（不含 `/`，如 `hello`），写入 `data`。
    fn write_regular_file_at_root(&mut self, name: &str, data: &[u8]) -> FsResult<()>;

    /// 在绝对路径 `path` 处创建或替换普通文件并写入 `data`。
    fn write_regular_file(&mut self, path: &str, data: &[u8]) -> FsResult<()> {
        let _ = (path, data);
        Err(FsError::Unsupported)
    }

    /// 删除绝对路径指向的 **普通文件**；目录删除见 [`ReadWriteFs::rmdir`].
    fn unlink(&mut self, path: &str) -> FsResult<()> {
        let _ = path;
        Err(FsError::Unsupported)
    }

    /// 删除空目录（`rmdir` / `unlinkat` + `AT_REMOVEDIR`）。
    fn rmdir(&mut self, path: &str) -> FsResult<()> {
        let _ = path;
        Err(FsError::Unsupported)
    }

    /// 从 `offset` 起写入 `data`；返回实际写入字节数。
    fn write_range(&mut self, path: &str, offset: u64, data: &[u8]) -> FsResult<usize> {
        let _ = (path, offset, data);
        Err(FsError::Unsupported)
    }

    /// 调整普通文件长度；缩短时丢弃 EOF 之后数据，增长时新区域读为零。
    fn truncate(&mut self, path: &str, len: u64) -> FsResult<()> {
        let _ = (path, len);
        Err(FsError::Unsupported)
    }

    /// 在绝对路径 `path` 处创建目录；`mode` 为 Linux `mkdir` 权限位（不含 umask）。
    fn mkdir(&mut self, path: &str, mode: u32) -> FsResult<()> {
        let _ = (path, mode);
        Err(FsError::Unsupported)
    }

    /// 修改绝对路径 `path` 的权限位；`mode` 取 Linux `chmod` 语义（`mode & 0o7777`）。
    fn chmod(&mut self, path: &str, mode: u32) -> FsResult<()> {
        let _ = (path, mode);
        Err(FsError::Unsupported)
    }

    /// 修改绝对路径 `path` 的 uid/gid；`None` 表示对应字段不修改（Linux `-1`）。
    fn chown(&mut self, path: &str, uid: Option<u32>, gid: Option<u32>) -> FsResult<()> {
        let _ = (path, uid, gid);
        Err(FsError::Unsupported)
    }

    /// 设置扩展属性；`name` 为完整属性名（如 `user.foo`）。
    fn setxattr(&mut self, path: &str, name: &str, value: &[u8]) -> FsResult<()> {
        let _ = (path, name, value);
        Err(FsError::Unsupported)
    }

    /// 读取扩展属性；`buf` 为空时返回所需长度。
    fn getxattr(&self, path: &str, name: &str, buf: &mut [u8]) -> FsResult<usize> {
        let _ = (path, name, buf);
        Err(FsError::Unsupported)
    }

    /// 列出扩展属性名；`buf` 为空时返回所需长度（含结尾 `\\0`）。
    fn listxattr(&self, path: &str, buf: &mut [u8]) -> FsResult<usize> {
        let _ = (path, buf);
        Err(FsError::Unsupported)
    }

    /// 删除扩展属性。
    fn removexattr(&mut self, path: &str, name: &str) -> FsResult<()> {
        let _ = (path, name);
        Err(FsError::Unsupported)
    }

    /// 将 `old_path` 重命名为 `new_path`（实现可限制为同父目录）。
    fn rename(&mut self, old_path: &str, new_path: &str) -> FsResult<()> {
        let _ = (old_path, new_path);
        Err(FsError::Unsupported)
    }

    /// 为已存在的 `existing_path` 创建硬链接 `new_path`。
    fn hardlink(&mut self, existing_path: &str, new_path: &str) -> FsResult<()> {
        let _ = (existing_path, new_path);
        Err(FsError::Unsupported)
    }

    /// 在 `link_path` 创建指向 `target` 的符号链接。
    fn symlink(&mut self, link_path: &str, target: &str) -> FsResult<()> {
        let _ = (link_path, target);
        Err(FsError::Unsupported)
    }

    /// 创建设备/套接字等特殊节点（`mknod(2)` 语义）。
    fn mknod(&mut self, path: &str, mode: u32, rdev: u32) -> FsResult<()> {
        let _ = (path, mode, rdev);
        Err(FsError::Unsupported)
    }

    /// 路径是否存在（RW 实现可覆盖，供单 RW 根卷统一读路径）。
    fn exists(&self, path: &str) -> FsResult<bool> {
        let _ = path;
        Err(FsError::Unsupported)
    }

    /// 路径元数据。
    fn metadata(&self, path: &str) -> FsResult<FsMetadata> {
        let _ = path;
        Err(FsError::Unsupported)
    }

    /// 读取整个普通文件。
    fn read(&self, path: &str) -> FsResult<Vec<u8>> {
        let _ = path;
        Err(FsError::Unsupported)
    }

    /// 从 `offset` 起读取。
    fn read_range(&self, path: &str, offset: u64, buf: &mut [u8]) -> FsResult<usize> {
        let _ = (path, offset, buf);
        Err(FsError::Unsupported)
    }

    /// 列出目录项。
    fn read_dir(&self, path: &str) -> FsResult<Vec<FsDirEntry>> {
        let _ = path;
        Err(FsError::Unsupported)
    }

    /// 读取符号链接目标。
    fn read_symlink(&self, path: &str) -> FsResult<Vec<u8>> {
        let _ = path;
        Err(FsError::Unsupported)
    }
}

/// 异步区间 I/O 占位（v1 未实现）。
pub trait FsAsyncIo {
    /// 异步从 `offset` 读取；v1 应返回 [`FsError::Unsupported`]。
    fn async_read_range(
        &self,
        path: &str,
        offset: u64,
        len: usize,
    ) -> FsResult<()> {
        let _ = (path, offset, len);
        Err(FsError::Unsupported)
    }

    /// 异步写入；v1 应返回 [`FsError::Unsupported`]。
    fn async_write_range(
        &mut self,
        path: &str,
        offset: u64,
        data: &[u8],
    ) -> FsResult<()> {
        let _ = (path, offset, data);
        Err(FsError::Unsupported)
    }
}

/// 将 `dyn ReadWriteFs` 装箱后的本地句柄，用于装入 [`SharedRwFs`]。
pub struct LocalRwFs(Box<dyn ReadWriteFs>);

impl LocalRwFs {
    /// 由具体 RW 实现构造本地包装。
    pub fn new(inner: Box<dyn ReadWriteFs>) -> Self { Self(inner) }
}

impl Deref for LocalRwFs {
    type Target = dyn ReadWriteFs;

    fn deref(&self) -> &Self::Target { &*self.0 }
}

impl DerefMut for LocalRwFs {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut *self.0 }
}

impl ReadWriteFs for LocalRwFs {
    fn mount_rw(&mut self, device: SharedBlockDevice) -> FsResult<()> {
        self.deref_mut().mount_rw(device)
    }

    fn is_mounted(&self) -> bool { self.deref().is_mounted() }

    fn write_regular_file_at_root(&mut self, name: &str, data: &[u8]) -> FsResult<()> {
        self.deref_mut().write_regular_file_at_root(name, data)
    }

    fn write_regular_file(&mut self, path: &str, data: &[u8]) -> FsResult<()> {
        self.deref_mut().write_regular_file(path, data)
    }

    fn unlink(&mut self, path: &str) -> FsResult<()> { self.deref_mut().unlink(path) }

    fn rmdir(&mut self, path: &str) -> FsResult<()> { self.deref_mut().rmdir(path) }

    fn write_range(&mut self, path: &str, offset: u64, data: &[u8]) -> FsResult<usize> {
        self.deref_mut().write_range(path, offset, data)
    }

    fn truncate(&mut self, path: &str, len: u64) -> FsResult<()> {
        self.deref_mut().truncate(path, len)
    }

    fn mkdir(&mut self, path: &str, mode: u32) -> FsResult<()> {
        self.deref_mut().mkdir(path, mode)
    }

    fn chmod(&mut self, path: &str, mode: u32) -> FsResult<()> {
        self.deref_mut().chmod(path, mode)
    }

    fn chown(&mut self, path: &str, uid: Option<u32>, gid: Option<u32>) -> FsResult<()> {
        self.deref_mut().chown(path, uid, gid)
    }

    fn setxattr(&mut self, path: &str, name: &str, value: &[u8]) -> FsResult<()> {
        self.deref_mut().setxattr(path, name, value)
    }

    fn getxattr(&self, path: &str, name: &str, buf: &mut [u8]) -> FsResult<usize> {
        self.deref().getxattr(path, name, buf)
    }

    fn listxattr(&self, path: &str, buf: &mut [u8]) -> FsResult<usize> {
        self.deref().listxattr(path, buf)
    }

    fn removexattr(&mut self, path: &str, name: &str) -> FsResult<()> {
        self.deref_mut().removexattr(path, name)
    }

    fn rename(&mut self, old_path: &str, new_path: &str) -> FsResult<()> {
        self.deref_mut().rename(old_path, new_path)
    }

    fn hardlink(&mut self, existing_path: &str, new_path: &str) -> FsResult<()> {
        self.deref_mut().hardlink(existing_path, new_path)
    }

    fn symlink(&mut self, link_path: &str, target: &str) -> FsResult<()> {
        self.deref_mut().symlink(link_path, target)
    }

    fn mknod(&mut self, path: &str, mode: u32, rdev: u32) -> FsResult<()> {
        self.deref_mut().mknod(path, mode, rdev)
    }

    fn exists(&self, path: &str) -> FsResult<bool> { self.deref().exists(path) }

    fn metadata(&self, path: &str) -> FsResult<FsMetadata> { self.deref().metadata(path) }

    fn read(&self, path: &str) -> FsResult<Vec<u8>> { self.deref().read(path) }

    fn read_range(&self, path: &str, offset: u64, buf: &mut [u8]) -> FsResult<usize> {
        self.deref().read_range(path, offset, buf)
    }

    fn read_dir(&self, path: &str) -> FsResult<Vec<FsDirEntry>> { self.deref().read_dir(path) }

    fn read_symlink(&self, path: &str) -> FsResult<Vec<u8>> { self.deref().read_symlink(path) }
}

// 与 LocalFs 相同：单核 bring-up 下由 Mutex 序列化；跨线程 Send 由调用方保证不数据竞争。
unsafe impl Send for LocalRwFs {}

/// 线程间共享的读写文件系统句柄（`Arc<Mutex<...>>`）。
pub type SharedRwFs = Arc<Mutex<LocalRwFs>>;

/// 单个文件系统实现的统一注册接口。`impl-*` crate 暴露一个 `'static` 实例（如 `&IMPL`）供聚合层登记。
///
/// 设计要点：
/// - `name` 与 `supported` 用于运行时打印与挂载前能力查询；
/// - `probe` 通过读取设备前若干字节返回它判断的 [`FsKind`]；不能识别返回 `Ok(None)`；
/// - `mount_ro` 必须实现；`mount_rw` 默认返回 `Unsupported`，由能写的 impl 覆盖。
pub trait FsImpl: Sync {
    /// 人类可读实现名（日志与 `supported_fs` 展示）。
    fn name(&self) -> &'static str;

    /// 静态能力表：该 impl 声明支持的所有 `(kind, access)` 组合。
    fn supported(&self) -> &'static [FsCapability];

    /// 是否支持指定的 kind 与访问模式（对 `supported()` 的便捷查询）。
    fn supports(&self, kind: FsKind, mode: FsAccessMode) -> bool {
        self.supported().iter().any(|c| c.kind == kind && c.access == mode)
    }

    /// 通过读取设备探测当前 impl 是否能识别该卷的文件系统类型。
    /// 默认实现返回 `Ok(None)`，例如内核 devfs 不挂在块设备上。
    fn probe(&self, _device: &SharedBlockDevice) -> FsResult<Option<FsKind>> { Ok(None) }

    /// 只读挂载并返回共享句柄。
    fn mount_ro(&self, device: SharedBlockDevice) -> FsResult<SharedFs>;

    /// 读写挂载；默认返回 [`FsError::Unsupported`]，由支持 RW 的 impl 覆盖。
    fn mount_rw(&self, _device: SharedBlockDevice) -> FsResult<SharedRwFs> {
        Err(FsError::Unsupported)
    }
}

/// 将 `dyn ReadOnlyFs` 装箱后的本地句柄，用于装入 [`SharedFs`]。
pub struct LocalFs(Box<dyn ReadOnlyFs>);

impl LocalFs {
    /// 由具体 RO 实现构造本地包装。
    pub fn new(inner: Box<dyn ReadOnlyFs>) -> Self { Self(inner) }
}

impl Deref for LocalFs {
    type Target = dyn ReadOnlyFs;

    fn deref(&self) -> &Self::Target { &*self.0 }
}

impl DerefMut for LocalFs {
    fn deref_mut(&mut self) -> &mut Self::Target { &mut *self.0 }
}

impl ReadOnlyFs for LocalFs {
    fn mount(&mut self, device: SharedBlockDevice) -> FsResult<()> {
        self.deref_mut().mount(device)
    }

    fn is_mounted(&self) -> bool { self.deref().is_mounted() }

    fn exists(&self, path: &str) -> FsResult<bool> { self.deref().exists(path) }

    fn metadata(&self, path: &str) -> FsResult<FsMetadata> { self.deref().metadata(path) }

    fn read(&self, path: &str) -> FsResult<Vec<u8>> { self.deref().read(path) }

    fn read_range(&self, path: &str, offset: u64, buf: &mut [u8]) -> FsResult<usize> {
        self.deref().read_range(path, offset, buf)
    }

    fn read_dir(&self, path: &str) -> FsResult<Vec<FsDirEntry>> { self.deref().read_dir(path) }

    fn read_symlink(&self, path: &str) -> FsResult<Vec<u8>> { self.deref().read_symlink(path) }

    fn boot_dump_all_paths(&self) { self.deref().boot_dump_all_paths(); }
}

// 当前阶段按单核串行访问；Send 用于跨线程传递 Arc，实际互斥由 Mutex 保证。
unsafe impl Send for LocalFs {}

/// 线程间共享的只读文件系统句柄（`Arc<Mutex<...>>`）。
pub type SharedFs = Arc<Mutex<LocalFs>>;

/// 内置样例 FS 的单元级自检（日志 + assert）；供外层聚合 crate 的 `test` 入口链式调用。
///
/// 不变量：使用本模块内私有样例 `ReadOnlyFs` 实现，不触碰块设备与全局挂载状态。
pub fn test() {
    logging::trace!("[fs-api] test begin");
    let fs = SampleFs;
    let text = fs.read_to_string("/hello.txt").expect("sample text");
    assert_eq!(text, "hello");
    let meta = fs.metadata("/hello.txt").expect("sample metadata");
    assert_eq!(meta.size, 5);
    assert_eq!(meta.inode, 2);
    assert_eq!(meta.nlink, 1);
    logging::trace!("[fs-api] test end");
}

// 纯内存样例：固定 `/hello.txt` 与 5 字节内容，仅用于 [`test`] 断言 trait 默认方法与错误分支。
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
                inode: 2,
                nlink: 1,
                uid: 0,
                gid: 0,
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

    fn read_dir(&self, path: &str) -> FsResult<Vec<FsDirEntry>> {
        if path == "/" {
            Ok(alloc::vec![FsDirEntry {
                name: String::from("hello.txt"),
                node_type: FsNodeType::File,
            }])
        } else {
            Err(FsError::NotFound)
        }
    }
}

use alloc::{boxed::Box, sync::Arc, vec::Vec};
use core::ops::{Deref, DerefMut};
use driver_block_api_v0::SharedBlockDevice;
use spin::Mutex;
use super::*;

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

    fn sync(&mut self) -> FsResult<()> { self.deref_mut().sync() }

    fn open_node(&mut self, path: &str) -> FsResult<FsNodeId> {
        self.deref_mut().open_node(path)
    }

    fn close_node(&mut self, node: FsNodeId) -> FsResult<()> {
        self.deref_mut().close_node(node)
    }

    fn metadata_node(&self, node: FsNodeId) -> FsResult<FsMetadata> {
        self.deref().metadata_node(node)
    }

    fn read_range_node(
        &self,
        node: FsNodeId,
        offset: u64,
        buf: &mut [u8],
    ) -> FsResult<usize> {
        self.deref().read_range_node(node, offset, buf)
    }

    fn write_range_node(
        &mut self,
        node: FsNodeId,
        offset: u64,
        data: &[u8],
    ) -> FsResult<usize> {
        self.deref_mut().write_range_node(node, offset, data)
    }

    fn truncate_node(&mut self, node: FsNodeId, len: u64) -> FsResult<()> {
        self.deref_mut().truncate_node(node, len)
    }

    fn create_tmpfile_node(
        &mut self,
        directory: &str,
        mode: u32,
        uid: u32,
        gid: u32,
    ) -> FsResult<FsNodeId> {
        self.deref_mut().create_tmpfile_node(directory, mode, uid, gid)
    }

    fn link_node(&mut self, node: FsNodeId, new_path: &str) -> FsResult<()> {
        self.deref_mut().link_node(node, new_path)
    }

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

struct SampleFs;

impl ReadOnlyFs for SampleFs {
    fn mount(&mut self, _device: SharedBlockDevice) -> FsResult<()> { Ok(()) }
    fn is_mounted(&self) -> bool { true }
    fn exists(&self, path: &str) -> FsResult<bool> { Ok(path == "/hello.txt") }
    fn metadata(&self, path: &str) -> FsResult<FsMetadata> {
        if path == "/hello.txt" {
            Ok(FsMetadata { node_type: FsNodeType::File, size: 5, mode: 0o644,
                            inode: 2, nlink: 1, uid: 0, gid: 0 })
        } else { Err(FsError::NotFound) }
    }
    fn read(&self, path: &str) -> FsResult<Vec<u8>> {
        if path == "/hello.txt" { Ok(b"hello".to_vec()) } else { Err(FsError::NotFound) }
    }
}

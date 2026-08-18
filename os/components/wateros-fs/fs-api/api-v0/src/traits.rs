use alloc::{string::String, vec::Vec};
use super::*;
use driver_block_api_v0::SharedBlockDevice;

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
        // 默认实现不猜测后端语义；未覆盖时显式报告 Unsupported，避免伪造 EOF。
        let _ = (path, offset, buf);
        Err(FsError::Unsupported)
    }

    /// 读取文件前缀，最多 `len` 字节（短于文件则截断）。
    ///
    /// 默认回退为整文件 [`read`] 后截断；实现方应覆盖为 [`read_range`] 以避免大文件全量分配。
    fn read_prefix(&self, path: &str, len: usize) -> FsResult<Vec<u8>> {
        // len 为零时结果仍必须为空，不能因为默认整文件回退而读取越界数据。
        let mut data = self.read(path)?;
        if data.len() > len {
            data.truncate(len);
        }
        Ok(data)
    }

    /// 读取整个文件并校验为 UTF-8 字符串。
    fn read_to_string(&self, path: &str) -> FsResult<String> {
        let data = self.read(path)?;
        // 非 UTF-8 内容转换失败并丢弃临时 Vec，不能把原始字节误当作字符串返回。
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

    /// 将该挂载上已提交给文件系统的脏数据和元数据同步到底层存储。
    ///
    /// 具体文件的页缓存由 VFS 句柄先行写回；本方法负责文件系统及块缓存层。
    fn sync(&mut self) -> FsResult<()> {
        Err(FsError::Unsupported)
    }

    /// 打开路径对应的稳定节点并持有一个后端 open 引用。
    ///
    /// 成功后调用方必须恰好调用一次 [`Self::close_node`]。rename 和 unlink 不得让该
    /// identity 指向其它文件；实现不支持稳定节点时返回 [`FsError::Unsupported`]。
    fn open_node(&mut self, path: &str) -> FsResult<FsNodeId> {
        let _ = path;
        Err(FsError::Unsupported)
    }

    /// 释放 [`Self::open_node`] 获取的后端 open 引用。
    fn close_node(&mut self, node: FsNodeId) -> FsResult<()> {
        let _ = node;
        Err(FsError::Unsupported)
    }

    /// 按稳定节点身份查询元数据。
    fn metadata_node(&self, node: FsNodeId) -> FsResult<FsMetadata> {
        let _ = node;
        Err(FsError::Unsupported)
    }

    /// 按稳定节点身份读取区间。
    fn read_range_node(
        &self,
        node: FsNodeId,
        offset: u64,
        buf: &mut [u8],
    ) -> FsResult<usize> {
        let _ = (node, offset, buf);
        Err(FsError::Unsupported)
    }

    /// 按稳定节点身份写入区间。
    fn write_range_node(
        &mut self,
        node: FsNodeId,
        offset: u64,
        data: &[u8],
    ) -> FsResult<usize> {
        let _ = (node, offset, data);
        Err(FsError::Unsupported)
    }

    /// 按稳定节点身份调整普通文件长度。
    fn truncate_node(&mut self, node: FsNodeId, len: u64) -> FsResult<()> {
        let _ = (node, len);
        Err(FsError::Unsupported)
    }

    /// 在 `directory` 所在文件系统中创建没有目录项的普通 inode，并持有一次 open 引用。
    /// 返回的节点必须由调用方通过 [`Self::close_node`] 释放；若从未发布，最后一次关闭时回收。
    fn create_tmpfile_node(
        &mut self,
        directory: &str,
        mode: u32,
        uid: u32,
        gid: u32,
    ) -> FsResult<FsNodeId> {
        let _ = (directory, mode, uid, gid);
        Err(FsError::Unsupported)
    }

    /// 将已打开且尚无名称的普通节点链接到 `new_path`。
    fn link_node(&mut self, node: FsNodeId, new_path: &str) -> FsResult<()> {
        let _ = (node, new_path);
        Err(FsError::Unsupported)
    }

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

# wateros-vfs — 聚合层公共 API

## 用途

描述根内核与 syscall 通过 `vfs` 依赖**实际使用**的导出符号（非 `api-v0` 全量 trait 目录）。

事实来源：`os/components/wateros-vfs/src/lib.rs`；根 `os/Cargo.toml` 中 `vfs = { package = "wateros-vfs", ... }`，经 `vfs-bridge` feature 启用。

## 顶层 re-export

```text
vfs::api              // wateros-vfs-api-v0 别名
vfs::active_impl      // 当前 VfsBackend（FsBridge 或 DummyBackend）
vfs::root             // 单根只读视图
vfs::mount            // RW 挂载会话
vfs::self_test        // 组合自检
vfs::fd               // [impl-fd-session] per-task fd
vfs::cwd              // [impl-fd-session] per-task cwd
vfs::mount_ns         // [impl-fd-session + bridge] 挂载命名空间转发
```

契约类型（自 `api` 再导出）：`VfsError`、`VfsResult`、`VfsBackend`、`VfsIoHandle`、`VfsOpenFlags`、`VfsMetadata`、`NormalizedPath`、`resolve_open_path` 等。

## `vfs::root`

| 符号 | 说明 |
|------|------|
| `read_view()` | `&'static impl SingleRootReadView` |

## `vfs::mount`

| 符号 | 说明 |
|------|------|
| `open_rw_session(kind)` | 按 `VfsFsKind` 打开 `Box<dyn RootRwSession>` |
| `supported_capabilities()` | 当前后端声明的能力列表 |

## 路径与挂载（`bridge-fs-api`）

| 函数 | 说明 |
|------|------|
| `mkdir_absolute` / `mkdir_at_current` | 创建目录 |
| `unlink_absolute` / `unlink_at_current` | 删文件或空目录 |
| `rename_absolute` | 重命名 |
| `chmod_absolute` / `chown_absolute` | 权限与属主 |
| `setxattr_absolute` / `getxattr_absolute` / `listxattr_absolute` / `removexattr_absolute` | xattr |
| `truncate_absolute` | 截断普通文件 |
| `symlink_absolute` / `read_symlink_absolute` | 符号链接 |
| `mknod_socket_absolute` | AF_UNIX bind 用 socket 节点 |
| `overwrite_absolute_file` | unlink + 写 + 页缓存驱逐 |
| `mount_ext4_block_at` / `mount_tmpfs_at` / `mount_cgroup_at` | 辅助卷挂载 |
| `mount_procfs_at` / `ensure_proc_mount_point` / `is_proc_mounted_at` | procfs |
| `mount_securityfs_at` / `mount_bind_at` / `move_mount_at` | 伪 FS 与 bind |
| `set_mount_propagation` | 传播类型（`MountPropagation`） |
| `remount_readonly_at` / `unmount_at` | remount / umount |
| `assert_path_writable` / `mount_statfs_magic` | 写权限与 magic |
| `reset_file_page_cache` | 批量刷回与回收页缓存 |

## `vfs::fd`（`impl-fd-session`）

| 符号 | 说明 |
|------|------|
| `PerTaskFdRegistry` | 每任务 fd 表 |
| `pipe_handle_pair` / `stream_pair_handle_pair` | pipe 与 socket pair |
| `PipeReadHandle` / `PipeWriteHandle` / `UnixStreamPairEnd` | 句柄类型 |
| `Flock` / `InodeKey` / `LOCK_*` | 建议性文件锁 |

## `vfs::cwd`（`impl-fd-session`）

| 符号 | 说明 |
|------|------|
| `PerTaskCwdRegistry` | 每任务 cwd |
| `resolve_for_current_task` | 相对路径 → 绝对路径 |
| `lookup_argv_for_task` / `lookup_exe_for_task` | procfs 回调数据源 |

## `vfs::api` 主要 trait（syscall 直接引用）

| 路径 | 典型消费者 |
|------|------------|
| `api::handle::{VfsIoHandle, VfsOpenOps, VfsOpenFlags}` | read/write/open/lseek |
| `api::fd::VfsFdSession` | fd 表操作 |
| `api::resolve::{resolve_open_path, resolve_against_cwd}` | openat 路径 |
| `api::path::{normalize_absolute_path, validate_root_file_name}` | 路径校验 |
| `api::namespace::VfsMountTable` | mount 表查询 |
| `api::error::{VfsError, VfsResult}` | errno 映射 |

## 自检

| 符号 | 说明 |
|------|------|
| `vfs::test()` | 串联 api-v0、dummy、bridge、fd、cwd、self_test |
| `vfs::self_test::run` | bring-up 烟囱（warn 不 panic） |

## 未通过聚合层导出的内容

- `impl-fs-bridge` / `impl-page-cache` 内部路由与缓存结构
- `impl-fd-session` 各 `*Handle` 实现细节（经 `fd` 模块有限导出）
- `wateros-fs` 类型（bridge 刻意不 re-export）

依赖方应使用 `vfs::api` 契约与聚合函数，而非直接依赖 `wateros-vfs-impl-fs-bridge`。

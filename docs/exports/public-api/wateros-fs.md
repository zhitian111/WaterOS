# wateros-fs 公共 API 快照

## 用途

列出根 crate **`wateros-fs`** 对外可见的主要入口与类型，便于与 **`os`** 依赖侧对齐。契约细节以各 **`api-v0`** crate 源码为准；完整上下文见 **`docs/guides/filesystem-current.md`**。

## 事实来源

- `os/components/wateros-fs/src/lib.rs`
- `os/components/wateros-fs/fs-api/api-v0/src/lib.rs`
- `os/components/wateros-fs/fs-devfs/devfs-api/api-v0/src/lib.rs`
- `os/components/wateros-fs/fs-devfs/devfs-impl/impl-kernel/src/lib.rs`
- `os/components/wateros-fs/fs-rootfs/rootfs-api/api-v0/src/lib.rs`
- `os/components/wateros-fs/fs-rootfs/rootfs-impl/impl-kernel/src/lib.rs`
- `os/components/wateros-fs/fs-impl/impl-ext4/src/{lib.rs,ro.rs,rw.rs,selftest.rs}`

## 聚合层（`wateros-fs` 根 crate）

| 项 | 说明 |
|----|------|
| **`init()`** | 打印 supported_fs；刷新 devfs；从 `registered_fs_impls()` 中 probe 选定 RO impl 注入 rootfs 后 `mount_default_root`；启动树与 devfs 路径日志 |
| **`test()`** | 调用 `api_v0::test()`；若已挂载则 `impl_ext4::ro_self_test`；再用 `pick_fs_impl(Ext4, ReadWrite)` 跑 `impl_ext4::rw_smoke_self_test` 并经只读句柄 `read("/hello")` 校验 |
| **`registered_fs_impls()`** | `&'static [&'static dyn FsImpl]`：cfg 静态拼接 `impl_ext4::IMPL`、`devfs::active_impl::IMPL` 与 **`procfs::active_impl::IMPL`** |
| **`supported_fs_summary()`** | `Vec<FsCapability>`：所有已注册 impl 的 `supported()` 扁平化 |
| **`pick_fs_impl(kind, mode)`** | `Option<&'static dyn FsImpl>`：注册表中首个 `supports(kind, mode)` 命中项 |
| **`pub mod api`** | 重导出 **`wateros-fs-api-v0`** |
| **`pub mod devfs` / `pub mod rootfs` / `pub mod procfs`** | 重导出子聚合 crate 的公开项 |
| **`pub use api_v0::*`** | `FsError`、`FsKind`、`FsAccessMode`、`FsCapability`、`FsImpl`、`ReadOnlyFs`、`ReadWriteFs`、`SharedFs`、`SharedRwFs` 等 |
| **`impl_ext4`** | 当 `feature = "impl-ext4"` 启用时再导出，便于上层直接调用其自检入口 |

## `wateros-fs-api-v0`

- **类型**：`FsError`、`FsResult`、`FsNodeType`、`FsMetadata`、`FsKind`、`FsAccessMode`、`FsCapability`。
- **`ReadOnlyFs`**：`mount` / `is_mounted` / `exists` / `metadata` / `read` / `read_prefix` / `read_to_string` / `boot_dump_all_paths`；并为 `LocalFs` 自身实现转发。
- **`ReadWriteFs`**：`mount_rw` / `is_mounted` / `write_regular_file_at_root`。
- **`FsImpl`**：`name() / supported() / supports(kind, mode) / probe(device) -> FsResult<Option<FsKind>> / mount_ro(device) -> FsResult<SharedFs> / mount_rw(device) -> FsResult<SharedRwFs>`。
- **句柄**：`LocalFs`、`LocalRwFs`；`SharedFs = Arc<Mutex<LocalFs>>`、`SharedRwFs = Arc<Mutex<LocalRwFs>>`。

## `wateros-fs-devfs`

- **`DevNodeType`**（`Block` / `Character` / `Unsupported`）、`DevNode`。
- **`DevFsManager`**：`refresh` / `set_dt_unsupported_paths` / `list_nodes` / `register_block_device` / `lookup_block_device` / `default_root_block_path`。
- **`active_impl`**：默认 **`impl-kernel`**（模块级 `refresh` / `list_nodes` / `lookup_block_device` / `default_root_block_path` / `set_dt_unsupported_paths`）。
- **`KernelDevFsImpl: FsImpl`**：`name = "devfs"`、`supported = &[(DevFs, ReadOnly)]`，作为 `pub static IMPL` 暴露，仅供注册表列示。

## `wateros-fs-procfs`

- **`ProcFsView`**：`exists` / `metadata` / `read` / `read_dir`（路径相对 procfs 挂载根）。
- **`active_impl::view()`**：默认 **`impl-kernel`** 的 **`KernelProcFs`**。
- **回调注册**（避免 fs↔vfs 环依赖）：`register_task_argv_lookup`、`register_task_exe_lookup`、`register_mount_list_lookup`。
- **`KernelProcFsImpl: FsImpl`**：`name = "procfs"`、`supported = &[(Other("procfs"), ReadOnly)]`。
- 架构说明：[`docs/architecture/wateros-procfs.md`](../../architecture/wateros-procfs.md)。

## `wateros-fs-rootfs`

- **`RootFsManager`**：`set_root_fs` / `root_fs` / `clear_root_fs` / `mount_root_from_block_path` / `current_root_device_path`。
- **`active_impl`**：默认 **`impl-kernel`**：模块级 **`mount_default_root`** / **`root_fs`** / **`current_root_device_path`** / **`set_active_fs_impl(&'static dyn FsImpl)`** / **`active_fs_impl()`**。

## `wateros-fs-impl-ext4`

- **`Ext4Fs`**：`ReadOnlyFs` 实现（基于 `ext4-view`）。
- **`Ext4FsRw`**：`ReadWriteFs` 实现（基于 `ext4plus`）。
- **`Ext4FsImpl` / `pub static IMPL`**：`FsImpl` 入口；`probe` 通过 superblock magic `0xEF53` 判断 ext2/3/4。
- **`ro_self_test(SharedFs)` / `rw_smoke_self_test(SharedRwFs, name, data)`**：供聚合层串接的自检入口。

## 维护要求

聚合导出或 `api-v0` 契约变更时，更新本文件与 **`docs/guides/filesystem-current.md`**。

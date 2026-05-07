# wateros-vfs 公共 API 快照

## 用途

描述一级组件 **`wateros-vfs`** 的聚合导出、`vfs-api-v0` 契约，以及可选 **`bridge-fs-api`** 下对 **`wateros-fs`** 的只读桥接与 RW 烟囱 API。

## 事实来源

- [`os/components/wateros-vfs/Cargo.toml`](../../os/components/wateros-vfs/Cargo.toml)
- [`os/components/wateros-vfs/src/lib.rs`](../../os/components/wateros-vfs/src/lib.rs)
- [`os/components/wateros-vfs/vfs-api/api-v0/src/lib.rs`](../../os/components/wateros-vfs/vfs-api/api-v0/src/lib.rs)
- [`os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/lib.rs`](../../os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/lib.rs)
- [`os/components/wateros-vfs/vfs-impl/impl-dummy/src/lib.rs`](../../os/components/wateros-vfs/vfs-impl/impl-dummy/src/lib.rs)

## 聚合层（`wateros-vfs` 根 crate）

| 项 | 说明 |
|----|------|
| **`pub mod api`** | 重导出 **`wateros-vfs-api-v0`** |
| **`pub use api_v0::*`** | `VfsError`、`VfsResult`、`VfsMetadata`、`VfsNodeType`、`normalize_absolute_path`、`NormalizedPath`、`SingleRootReadView`、`RootRwSession` 等 |
| **`pub mod dummy`** | 重导出 **`wateros-vfs-impl-dummy`**（`DummyRootView`、`DummyRwSession`） |
| **`pub mod bridge`** | 仅 **`feature = "bridge-fs-api"`** 时存在；重导出 **`wateros-vfs-impl-fs-bridge`** |
| **`test()`** | 调用 `api_v0::test`、`impl_dummy::test`；启用 bridge 时另跑 `impl_fs_bridge::test` |

## Feature

| Feature | 说明 |
|---------|------|
| **`default`** | `api-v0` + **`bridge-fs-api`**：默认即链接 **`wateros-vfs-impl-fs-bridge`**（依赖 **`wateros-fs`**），并仍通过硬依赖保留 **`impl-dummy`** 供占位根视图。 |
| **`api-v0`** | 向下传递 **`impl-dummy/api-v0`** 与（在启用 bridge 时）**`impl-fs-bridge?/api-v0`**。 |
| **`bridge-fs-api`** | `dep:impl-fs-bridge` + **`impl-fs-bridge/bridge-fs-api`**。 |
| **`impl-dummy`** | 当前为空数组占位 feature 名；与 **`impl-dummy`** crate 依赖并存，用于命名对齐。 |

根 crate **`wateros`** 在 **`default`** 下启用 **`vfs-bridge`** → **`vfs/bridge-fs-api`**；**`qemu-riscv64-opensbi`** 亦包含 **`vfs-bridge`**。若对 **`wateros-vfs`** 使用 **`default-features = false`** 且未开 **`bridge-fs-api`**，则聚合层无 **`pub mod bridge`**，仅剩 **`api`**、**`dummy`** 与根 **`pub use api_v0::*`**。

## `wateros-vfs-api-v0`

- **错误与结果**：`VfsError`、`VfsResult`。
- **元数据**：`VfsNodeType`、`VfsMetadata`（与 `fs-api` 语义对齐，类型独立，便于桥接映射）。
- **路径**：`normalize_absolute_path`、`NormalizedPath`。
- **Trait**：`SingleRootReadView`（`exists` / `metadata` / `read` / 默认 `read_prefix` / `read_to_string` / `boot_dump_all_paths`）；`RootRwSession`（`write_regular_file_at_root`）。

## `wateros-vfs-impl-fs-bridge`（`bridge-fs-api`）

仅通过 **`wateros-fs`** 聚合公开 API 访问根卷与 devfs，不修改 fs 实现源码。

| 项 | 说明 |
|----|------|
| **`FsBridge`** | 实现 `SingleRootReadView`：路径规范化后委托 `rootfs::active_impl::root_fs()` 上的 `ReadOnlyFs` |
| **`MountedRwSession`** | 包装 `SharedRwFs`，实现 `RootRwSession` |
| **`validate_root_file_name`** | 根目录文件名校验（无 `/`、非空） |
| **`supported_fs_capabilities` / `pick_fs_impl`** | 委托 `fs::supported_fs_summary` / `fs::pick_fs_impl` |
| **`mount_rw_session` / `rw_write_root_verify_via_ro`** | 与 `wateros-fs` 聚合层 `test()` 中 RW 段同构的 RW 挂载与只读读回校验 |
| **`list_dev_nodes` / `default_root_block_path`** | 委托 `devfs::active_impl` |
| **`pub use fs::{devfs, rootfs, FsAccessMode, FsCapability, FsImpl, FsKind}`** | 便于调用方使用与 fs 一致的类型 |

## `wateros-vfs-impl-dummy`

- **`DummyRootView`**：实现 `SingleRootReadView`；路径合法时卷访问返回 `NotMounted`。
- **`DummyRwSession`**：实现 `RootRwSession`，返回 `Unsupported`。

## 维护要求

契约、聚合导出、默认 feature 或 bridge 行为变化时，同步更新本文件、[`docs/exports/features/wateros-vfs.md`](./features/wateros-vfs.md) 与 [`docs/architecture/snapshot.md`](../architecture/snapshot.md)。

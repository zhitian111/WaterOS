# wateros-vfs 公共 API 快照

## 用途

描述一级组件 **`wateros-vfs`**：`vfs-api-v0` 基本能力契约、聚合层组合接口，以及 feature `bridge-fs-api` / `fd-session` 下的实现。

## 事实来源

- [`os/components/wateros-vfs/Cargo.toml`](../../os/components/wateros-vfs/Cargo.toml)
- [`os/components/wateros-vfs/src/lib.rs`](../../os/components/wateros-vfs/src/lib.rs)
- [`os/components/wateros-vfs/vfs-api/api-v0/`](../../os/components/wateros-vfs/vfs-api/api-v0/)

---

## 一、`vfs-api-v0` 基本能力（契约层）

本 crate **不** 依赖 `wateros-fs`。按能力域拆分模块：

| 模块 | 能力 |
|------|------|
| `error` | `VfsError`、`VfsResult`（含 `BadFd`、`WouldBlock`、`BrokenPipe`、`NoTask`） |
| `path` | `normalize_absolute_path`、`NormalizedPath`、`validate_root_file_name` |
| `meta` | `VfsNodeType`、`VfsMetadata`、`VfsDirEntry` |
| `kind` | `VfsFsKind`、`VfsAccessMode`、`VfsCapability` |
| `root_read` | `trait SingleRootReadView` |
| `rw_session` | `trait RootRwSession` |
| `mount` | `trait VfsMountOps` |
| `dev` | `trait VfsDevInventory`、`VfsDevNode` |
| `resolve` | `resolve_against_cwd` |
| `handle` | `trait VfsIoHandle`、`trait VfsFileHandle`、`trait VfsOpenOps`、`VfsOpenFlags` |
| `fd` | `trait VfsFdSession`、`VfsFd`、`VFS_STDIN_FD` 等常量 |
| `namespace` | `trait VfsMountTable`（占位） |
| `backend` | `trait VfsBackend`（路径/挂载/设备/打开；**不含** per-task fd 表） |

`impl-*` **只实现**这些 trait，不得 `pub use wateros-fs::*`。

---

## 二、聚合层组合接口（`wateros-vfs` 根 crate）

| 模块 / 项 | 说明 |
|-----------|------|
| **`pub use api_v0 as api`** | 重导出契约层 |
| **`active_impl::backend()`** | 当前 feature 选中的 `VfsBackend`（`bridge-fs-api` → `FsBridge`，否则 `DummyBackend`） |
| **`root::read_view()`** | `&'static impl SingleRootReadView` |
| **`mount::open_rw_session(kind)`** | `Box<dyn RootRwSession>` |
| **`mount::supported_capabilities()`** | 已注册后端能力列表 |
| **`fd`**（`fd-session`） | `registry()`、`with_current_io`、`alloc_fd`、`close_fd`、`self_test` |
| **`self_test::rw_write_root_verify_via_ro`** | RW 写后 RO 读回校验 |
| **`self_test::run()`** | 组合自检（`vfs::test()` 内调用） |
| **`test()`** | `api::test` + dummy + bridge + `fd::self_test` + `self_test::run` |
| **`dummy`**（`#[doc(hidden)]`） | 占位 impl，供 workspace 独立编译 |

**已移除：** `VfsFdTable` 作为 `VfsBackend` 子 trait；per-task fd 由 **`fd`** 模块与 **`impl-fd-session`** 承载。

---

## Feature

| Feature | 说明 |
|---------|------|
| **`default`** | `api-v0` + `bridge-fs-api` |
| **`api-v0`** | 向下传递子 crate `api-v0` |
| **`bridge-fs-api`** | 启用 `impl_fs_bridge`（依赖 `wateros-fs`） |
| **`fd-session`** | 启用 `impl-fd-session`、聚合层 `fd` 模块 |
| **`impl-riscv64` / `impl-loongarch64`** | 为 fd-session 传递平台 console / ipc / task feature |

根 **`wateros`** 在 `qemu-riscv64-opensbi` 下启用 `vfs-bridge`、`vfs/fd-session`、`vfs/impl-riscv64`，并传递 `mm/vfs-root-read` 使 ELF 装载经 `vfs::root::read_view()`。

---

## 维护要求

契约或组合接口变化时，同步本文件、[`features/wateros-vfs.md`](./features/wateros-vfs.md) 与 [`docs/architecture/snapshot.md`](../architecture/snapshot.md)。

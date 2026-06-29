# wateros-vfs — 新增 impl 指南

## 用途

说明在 `wateros-vfs` 下新增或替换实现时需要改动的 `Cargo.toml`、feature 链与必须实现的 trait。

## 目录约定

```text
wateros-vfs/
  vfs-api/api-v0/          # 契约（trait、错误、路径）
  vfs-impl/impl-<name>/    # 具体后端
  src/lib.rs               # 聚合与 active_impl 选择
```

## 新增 VFS 后端（`vfs-impl/impl-*`）

### 1. 创建 crate

- 路径：`vfs-impl/impl-<name>/`，`Cargo.toml` 中 `package = "wateros-vfs-impl-<name>"`。
- 依赖：`wateros-vfs-api-v0`（`api-v0` feature）。
- 将成员加入 `wateros-vfs/Cargo.toml` 的 `[workspace].members`。

### 2. 实现契约

核心入口 trait 为 [`VfsBackend`](../../../os/components/wateros-vfs/vfs-api/api-v0/src/backend.rs)，为以下 supertrait 的组合：

| Trait | 文件 | 职责 |
|-------|------|------|
| `SingleRootReadView` | `root_read.rs` | 单根只读 `exists`/`metadata`/`read` |
| `VfsMountOps` | `mount.rs` | `mount_rw_session`、能力枚举 |
| `VfsDevInventory` | `dev.rs` | `/dev` 节点列表 |
| `VfsOpenOps` | `handle.rs` | `open` → `VfsIoHandle` |
| `VfsMountTable` | `namespace.rs` | 挂载点解析（若需要多卷） |

另按需实现：

- `VfsFdSession`（`fd.rs`）：在 `impl-fd-session` 或独立 crate 中提供 per-task fd 表。
- `VfsIoHandle`（`handle.rs`）：具体文件的 read/write/seek。

参考实现：

- **占位**：`vfs-impl/impl-dummy`（最小 `VfsBackend`，无 bridge）。
- **主线**：`vfs-impl/impl-fs-bridge`（委托 `wateros-fs`，含 `impl-page-cache`）。

### 3. 接入聚合层

在 `wateros-vfs/Cargo.toml`：

```toml
[dependencies]
impl_<name> = { path = "./vfs-impl/impl-<name>/", package = "wateros-vfs-impl-<name>", optional = true }

[features]
my-impl = ["dep:impl_<name>", "impl_<name>/api-v0"]
```

在 `src/lib.rs` 的 `active_impl` 模块用 `#[cfg(feature = "...")]` 选择 `backend()` 返回值。

### 4. 根 `wateros` 传递

若新后端需从内核主线选用，在 `os/Cargo.toml` 的 `vfs` 依赖上增加 feature 传递，并在 `vfs-bridge` 或等价 feature 组中声明默认组合。

## 新增 fd 会话实现

-  crate 路径建议：`vfs-impl/impl-fd-session`（扩展现有）或 `impl-<name>-fd`。
-  实现 `VfsFdSession` + 各类 `VfsIoHandle`。
-  feature：`impl-fd-session = ["dep:impl_fd_session", ...]`，聚合层导出 `vfs::fd`。

## 页缓存

- `vfs-impl/impl-page-cache` 为 bridge 专用；新后端若需缓存，可复用 `global_cache(generation)` 或自建层。
- 挂载代次须与 `fs::rootfs::active_impl::mount_generation()` 对齐（见 bridge 源码）。

## 自检约定

- 在 impl crate 提供 `pub fn test()`（可选）。
- 聚合 `wateros-vfs::test()` 内按 feature 调用。
- bring-up 烟囱放在 `self_test` 模块，失败用 `log::warn!` 而非 panic。

## 常见陷阱

- **不要**在 `api-v0` 中依赖 `wateros-fs`；跨层桥接放在 `impl-fs-bridge`。
- `VfsError` 与 `FsError` 映射集中在 bridge，新后端应统一映射而非扩散。
- per-task 状态须与 `wateros-task` 的 tid 生命周期挂钩（参考 `PerTaskFdRegistry`）。

# wateros-fs — 新增 impl 指南

## 用途

说明在 `wateros-fs` 下新增块文件系统或伪 FS 实现时需要修改的文件、feature 与 `FsImpl` 契约。

## 目录约定

```text
wateros-fs/
  fs-api/api-v0/           # FsImpl、ReadOnlyFs、ReadWriteFs
  fs-impl/impl-<name>/     # 块 FS 或 devfs 适配
  fs-devfs/ fs-procfs/ fs-rootfs/   # 子系统聚合（各自 api-v0 + impl-*）
  src/lib.rs               # registered_fs_impls 注册表
```

## 新增块文件系统（`fs-impl/impl-*`）

### 1. 创建 crate

- 路径：`fs-impl/impl-<name>/`。
- 依赖：`wateros-fs-api-v0`、`driver_block_api_v0`（若需块设备）。
- 加入 `wateros-fs/Cargo.toml` 的 `[workspace].members` 与可选 `[dependencies]`。

### 2. 实现 `FsImpl`

静态导出 `pub static IMPL: ...`，实现：

| 方法 | 说明 |
|------|------|
| `name()` | 日志与探测标识 |
| `supported()` | `&[FsCapability]` 静态表 |
| `supports(kind, mode)` | 是否处理该组合 |
| `probe(device)` | 读 superblock 等，返回 `Ok(Some(FsKind))` 或 `Ok(None)` |
| `mount_ro(device)` | `Box<dyn ReadOnlyFs>` 或 `SharedFs` 工厂 |
| `mount_rw(device)` | `SharedRwFs` 工厂（若支持 RW） |

### 3. 实现卷 trait

- **只读**：`ReadOnlyFs` — `mount`、`exists`、`metadata`、`read`、`read_range`、`read_dir`、`read_symlink`。
- **读写**：`ReadWriteFs: Send` — 在 RO 基础上增加 `write_range`、`mkdir`、`unlink`、`chmod`、`xattr`、`rename` 等；未支持项可保留 trait 默认 `Unsupported`。

参考：

- `fs-impl/impl-another-ext4`（默认，vendored `another_ext4`）
- `fs-impl/impl-ext4-rs`（回退，`ext4_rs`）
- `fs-impl/impl-ext4`（ext4plus，`ro.rs`/`rw.rs`）

### 4. 注册到聚合层

在 `wateros-fs/src/lib.rs` 的 `registered_fs_impls()` 静态表中追加：

```rust
#[cfg(feature = "impl-<name>")]
&impl_<name>::IMPL,
```

在 `Cargo.toml` 增加：

```toml
impl-<name> = { path = "./fs-impl/impl-<name>/", optional = true }
impl-<name> = ["dep:impl_<name>"]
```

### 5. 与 rootfs 协作

- `fs::init()` 遍历 `registered_fs_impls()` probe 根块设备，调用 `rootfs::active_impl::set_active_fs_impl`。
- bring-up 调用 `mount_default_root_rw()` 使用已注入的 impl。
- 辅助挂载走 `rootfs::active_impl::mount_aux_*_from_block_path`。

## 新增伪文件系统（devfs / procfs 模式）

1. 在 `fs-<name>/` 下建聚合 crate：`api-v0` 定义视图 trait，`impl-kernel` 实现。
2. 若需列入 `supported_fs` 探测，在 `fs-impl/impl-<name>` 提供 `FsImpl`（如 `impl-devfs`）。
3. 聚合 `fs-<name>/src/lib.rs` 用 feature 选择 `active_impl`（kernel 优先于 dummy）。

参考：`fs-procfs/procfs-api/api-v0` 的 `ProcFsView` 与 `procfs-impl/impl-kernel`。

## Feature 传递链

新增 impl 时同步：

- `wateros-fs` 的 `api-v0` feature 列表中加入 `impl-<name>?/api-v0`。
- 根 `os/Cargo.toml` 的 `fs` 依赖 default-features 或显式 feature。
- 若 VFS 需可见，确认 `wateros-vfs` 的 `bridge-fs-api` 仍只依赖 `wateros-fs` 聚合 API。

## 自检约定

- `api-v0::test()`：契约级单元测试。
- impl crate：`rw_self_test` / `rw_mkdir_verify` 等（见 `impl-ext4`）。
- 聚合 `fs::test()` 在根 RW 挂载后调用 ext4 自检。

## 常见陷阱

- `ReadWriteFs` 实现类型须为 `Send`（`SharedRwFs` 跨任务共享）。
- probe 顺序由 `registered_fs_impls()` 表顺序决定；更具体的 impl 应排在前面。
- 勿在 `api-v0` 中引用具体 FS 库；块格式代码留在 `fs-impl`。
- 修改根卷挂载后须 bump `mount_generation`（rootfs impl-kernel 已处理），页缓存依赖此代次。

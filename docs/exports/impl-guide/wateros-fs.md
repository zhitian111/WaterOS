# wateros-fs 新增 impl 指南

## 用途与背景

指导在 **`wateros-fs`** 工作区内新增根卷 / devfs / rootfs 的具体 impl 时的检查步骤。当前主线为 **`impl-ext4` + `fs-devfs` impl-kernel + `fs-rootfs` impl-kernel**；整体数据流与边界说明见 **`docs/guides/filesystem-current.md`**。

## 新增根卷 impl（`fs-impl/impl-*`）的基本步骤

1. 创建新 crate（如 `fs-impl/impl-fat32/`），加入 **`wateros-fs` workspace `members`**。
2. 实现读路径：为某个内部类型实现 **`wateros-fs-api-v0::ReadOnlyFs`**（必要时覆盖 **`boot_dump_all_paths`**）。
3. 如支持写：再实现 **`ReadWriteFs`**（`mount_rw` / `write_regular_file_at_root`）。
4. 暴露 **`pub static IMPL: YourFsImpl`** 与 **`impl FsImpl for YourFsImpl`**：
   - **`name()`**：可读名（写入 supported_fs 日志）；
   - **`supported()`**：返回 `&'static [FsCapability]`；只支持读就只列 `(kind, ReadOnly)`；
   - **`probe(&device)`**：尝试根据设备前若干字节判断 `Some(FsKind::*)`，否则 `None`；
   - **`mount_ro(device)`** / **`mount_rw(device)`**：返回 `SharedFs` / `SharedRwFs`。
5. 在 **`wateros-fs/Cargo.toml`** 增加 `optional` 依赖与 feature；在 **`src/lib.rs`** 的 `registered_fs_impls()` 静态表中按 `cfg(feature = "...")` 加入 `&your_impl::IMPL`。
6. 用 **`pick_fs_impl(kind, mode)`** 而不是直接 `impl_ext4::*` 调用具体 impl，保持架构层整洁。

## 与 `wateros-vfs` 的边界

新增 impl 时应明确：当前 **`ReadOnlyFs` / `ReadWriteFs`** 栈与 **`wateros-vfs`** 未合并；若未来桥接，需在 **`docs/guides/filesystem-current.md`** 与 **`docs/exports/features/wateros-vfs.md`** 侧同步说明。

## 通用检查清单

- 新 impl 目录是否加入 **`wateros-fs` workspace `members`**
- impl crate 是否依赖正确的 **`api-v0`**
- impl 是否暴露 `pub static IMPL` 并实现 **`FsImpl`**（含 `name` / `supported` / `probe` / `mount_ro` / `mount_rw`）
- 组件根 **`Cargo.toml`** 是否新增 **feature** 并向下传递
- 聚合 **`src/lib.rs`** 的 `registered_fs_impls()` 是否新增了 `cfg(feature = ...)` 静态项
- **`docs/guides/filesystem-current.md`**、**`docs/exports/features/wateros-fs.md`**、**`docs/exports/public-api/wateros-fs.md`** 是否已同步更新

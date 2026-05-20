# wateros-fs 功能快照

## 用途

记录 **`wateros-fs`** 一级组件在默认 feature 下的能力边界，便于与路线图和实现对照。详细叙述见 **`docs/guides/filesystem-current.md`**。

## 事实来源

- `os/components/wateros-fs/Cargo.toml`
- `os/components/wateros-fs/src/lib.rs`
- `fs-api`、`fs-devfs`、`fs-rootfs`、`fs-impl/impl-ext4`（`ro` / `rw` / `selftest` 模块）子 crate 源码

## 当前已具备能力

- **能力描述与注册表**：`fs-api` 提供 **`FsKind` / `FsAccessMode` / `FsCapability` / `FsImpl`**；聚合层 `wateros-fs` 通过 **`registered_fs_impls()`**、**`supported_fs_summary()`**、**`pick_fs_impl(kind, mode)`** 暴露当前内核支持的 FS 列表，启动期打印 **`[fs] supported: ...`**。
- **devfs（kernel）**：按块设备枚举生成 **`/dev/vblkN`** 路径，**`lookup_block_device`**、**`default_root_block_path`**、**`list_nodes`**；通过 **`KernelDevFsImpl: FsImpl`** 在注册表中登记 **`(DevFs, ReadOnly)`**。
- **根卷（kernel rootfs）**：由聚合层按 `probe` 命中选择的 **`FsImpl`** 注入后，从默认块设备路径 **挂载 ext4 RO**，全局保存 **`SharedFs`**。
- **ext4 单一 impl**：**RO** 由 `ext4-view` 承载、**RW** 由 `ext4plus`（beta）承载；通过 `Ext4FsImpl` 的 **`probe`**（superblock magic）与 **`mount_ro`** / **`mount_rw`** 对外提供。
- **FS 契约（fs-api）**：**`ReadOnlyFs`**（`exists` / `metadata` / `read` / **`read_range`** / `boot_dump_all_paths`）、**`ReadWriteFs`**（`mount_rw` / `write_regular_file_at_root` / **`write_range`**）；**`FsAsyncIo`** 占位；句柄 **`SharedFs`** / **`SharedRwFs`**。
- **rootfs**：**`mount_generation()`** 供 VFS 页缓存失效；每次成功根挂载递增。
- **启动调试**：挂载成功后打印 **`/`**、devfs 节点，并对 ext4 根做 **`[fs::boot-tree]`** 路径 DFS；自检包括读固定文本/ELF 头与在根写入 `/hello` 后用只读句柄读回校验。

## 明确未覆盖

- 写回稳定性（`ext4plus` beta，无完整 journal）。
- 多挂载点、与 **`wateros-vfs`** 的统一 syscall 路径。
- devfs 侧字符设备节点填充（API 已预留类型）。
- `FsKind` 不区分 ext2/3/4，按 ext4 统一归并。

## 维护要求

行为或默认 feature 变化时，同步更新本文件与 **`docs/guides/filesystem-current.md`**、**`docs/exports/public-api/wateros-fs.md`**。

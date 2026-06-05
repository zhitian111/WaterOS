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
- **procfs（kernel）**：**`wateros-fs/fs-procfs`** 提供 **`ProcFsView`**；**`KernelProcFsImpl: FsImpl`** 登记 **`(Other("procfs"), ReadOnly)`**；生成 `/proc/<pid>/{stat,status,cmdline}`、`/proc/meminfo`、`/proc/mounts`（经 VFS 挂载表回调）。架构见 [`docs/architecture/wateros-procfs.md`](../../architecture/wateros-procfs.md)。
- **procfs（kernel）**：**`wateros-fs/fs-procfs`** 提供 **`ProcFsView`**；**`KernelProcFsImpl: FsImpl`** 登记 **`(Other("procfs"), ReadOnly)`**；生成 `/proc/<pid>/{stat,status,cmdline}`、`/proc/meminfo`、`/proc/mounts`（经 VFS 挂载表回调）。
- **procfs（kernel）**：**`wateros-fs/fs-procfs`** 提供 **`ProcFsView`**；**`KernelProcFsImpl: FsImpl`** 登记 **`(Other("procfs"), ReadOnly)`**；生成 `/proc/<pid>/{stat,status,cmdline}`、`/proc/meminfo`、`/proc/mounts`（经 VFS 挂载表回调）。架构见 [`docs/architecture/wateros-procfs.md`](../../architecture/wateros-procfs.md)。
- **根卷（kernel rootfs）**：`fs::init` 仅 **probe + 注入 `FsImpl`**；bring-up 总线调用 **`mount_default_root_rw`**，从默认块设备 **只挂载 ext4 RW**（`ext4plus`），全局保存 **`SharedRwFs`**；VFS 读路径与 `mkdir`/`write` 共用该句柄。
- **ext4 单一 impl**：**RO** 由 `ext4-view`（`mount_ro`，非 bring-up 主路径）、**RW** 由 `ext4plus`（beta）承载；RW 句柄同时实现读路径（`exists` / `metadata` / `read` / `read_range` / `read_dir`）。
- **FS 契约（fs-api）**：**`ReadOnlyFs`**、**`ReadWriteFs`**（含 RW 读路径默认 `Unsupported`、ext4 覆盖）；**`FsAsyncIo`** 占位；句柄 **`SharedFs`** / **`SharedRwFs`**。
- **rootfs**：**`mount_generation()`** 供 VFS 页缓存失效；每次成功根挂载递增。
- **启动调试**：挂载成功后打印 **`/`**、devfs 节点，并对 ext4 根做 **`[fs::boot-tree]`** 路径 DFS；自检包括读固定文本/ELF 头与在根写入 `/hello` 后用只读句柄读回校验。

## 明确未覆盖

- 写回稳定性（`ext4plus` beta，无完整 journal）。
- 完整 Linux `/proc` 语义（`/proc/self`、完整 stat 字段、线程 tid 目录等）；见 [`docs/architecture/wateros-procfs.md`](../../architecture/wateros-procfs.md)。
- devfs 侧字符设备节点填充（API 已预留类型）。
- `FsKind` 不区分 ext2/3/4，按 ext4 统一归并。

## 维护要求

行为或默认 feature 变化时，同步更新本文件与 **`docs/guides/filesystem-current.md`**、**`docs/exports/public-api/wateros-fs.md`**。

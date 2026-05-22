# wateros-vfs 功能快照

## 当前状态

- **`vfs-api-v0`**：VFS 模块基本能力（路径、单根只读、RW 会话、挂载、dev 视图、`VfsIoHandle` / `VfsFdSession`、多挂载占位 trait）；**不**依赖 `wateros-fs`。
- **`vfs-impl-dummy`**：实现 `VfsBackend` 路径/挂载相关 trait 的占位后端。
- **`vfs-impl-fs-bridge`**（`bridge-fs-api`）：经 `wateros-fs` 实现当前可落地的 trait 子集；`open` 按 `wateros-base-config::fs::FILE_LARGE_THRESHOLD` 分流 **`BufferedFileHandle`** / **`PagedFileHandle`**。
- **`vfs-impl-page-cache`**：全局共享页缓存（Direct、`FILE_PAGE_SIZE` 行、LRU 容量与可选预取步长）；键 `(mount_generation, path)`。
- **`vfs-impl-fd-session`**（`fd-session`）：per-task fd 表、**per-task cwd**（`PerTaskCwdRegistry`）、控制台与 pipe 的 `VfsIoHandle` 实现。
- **聚合层**：`active_impl` + `root` / `mount` / `self_test` / `fd` / **`cwd`** 组合对外接口。

## 根 crate 接线

- **`wateros`** 在 **`qemu-riscv64-opensbi`** 下启用 **`vfs-bridge`**、**`vfs/fd-session`**、**`vfs/impl-riscv64`**，在 `fs::init` / `fs::test` 之后调用 **`vfs::test()`**（含 `fd::self_test` 与 RW 读回烟囱）。
- **`wateros-mm`** 在相同 feature 下启用 **`vfs-root-read`**，`from_elf_path` 经 **`vfs::root::read_view()`** 读 ELF。
- **`wateros-syscall`** 经 **`fd-session`** feature 依赖 **`vfs::fd`** 与 **`vfs::cwd`** 完成 `read` / `write` / `close` / `pipe2` / `getcwd` / `chdir` / `mkdirat`；`open` 经 **`resolve_open_path`** 相对 cwd 解析。

## 工作区说明

- **`[workspace].members`** 含 `api-v0`、`impl-dummy`、`impl-fd-session`、`impl-page-cache`；`impl-fs-bridge` 由 `wateros` 在 RISC-V 目标下路径依赖编译。

## 后续关注点

- 文件 **Async I/O**（`FILE_IO_MODE::Async`）与跨 fd 细粒度页锁。
- 任务退出时 fd 批量关闭（cwd 已在 `waitpid` reap 与显式 reap 路径 **`drop_task_cwd`**）、`dup` / fork 时 **`copy_cwd_from_parent`** 接线。
- 多挂载与 vnode 扩展 `vfs-api` 占位模块。

## 维护

能力或组合接口变化时同步 [`public-api/wateros-vfs.md`](../public-api/wateros-vfs.md) 与本文件。

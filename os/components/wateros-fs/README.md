# wateros-fs

[项目首页](../../../README.md) · [内核工程](../../README.md) · [系统架构](../../../README.md#系统架构)

`wateros-fs` 是 WaterOS 的文件系统聚合层。它统一 [`api_v0::FsImpl`] 注册、启动期根卷探测，
并转发 devfs / rootfs 子 crate。语义契约：`init` 刷新 devfs 并探测块设备、注入 rootfs 所选
impl（**不**挂载根卷）；bring-up 通过 `mount_default_root_rw` 挂载单一 ext4 RW 视图。

## 模块分层

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 聚合门面 | `src/lib.rs` | `FsImpl` 注册表、`init`、`mount_default_root_rw`、`pick_fs_impl`、`supported_fs_summary`。 |
| FS API | `fs-api/api-v0/` | 错误与能力枚举、只读/可写根卷 trait、`FsImpl` 聚合注册面与 `SharedFs` / `SharedRwFs`。 |
| devfs | `fs-devfs/` | 设备文件系统：节点刷新、块设备查找与默认根块路径。 |
| procfs | `fs-procfs/` | 进程信息伪文件系统。 |
| rootfs | `fs-rootfs/` | 当前根卷共享句柄与挂载入口。 |
| ext4 实现 | `fs-impl/impl-{another-ext4,ext4-rs,ext4}/` | 三种 ext4 后端，默认 `impl-another-ext4`。 |
| ramfs 实现 | `fs-impl/impl-ramfs/` | 物理页 payload 后端 ramfs；tmpfs 策略层复用。 |
| 适配/占位 | `fs-impl/impl-devfs/`、`fs-impl/impl-dummy/` | devfs 的 fs impl 适配与无硬件占位。 |

## 实现说明

- `init` 只刷新 devfs、探测根块设备并注入活动 impl，**不**挂载根卷；`mount_default_root_rw`
  才挂载单一 ext4 RW 视图；`test` 依赖该挂载状态。
- 默认 ext4 RW 实现为 `impl-another-ext4`；`impl-ext4-rs` / `impl-ext4` 保留为回退 feature。
  同时启用多个 ext4 后端会在编译期报错（`compile_error`）。
- `registered_fs_impls` 按特性宏静态拼接各 `'static FsImpl`（ext4 族 + ramfs + devfs +
  procfs）；`pick_fs_impl(kind, mode)` 在注册表中匹配一条支持该 `(FsKind, FsAccessMode)` 的
  impl。
- devfs / procfs / rootfs 各自是“聚合 crate → API → impl”结构，`impl-kernel` 优先于互斥的
  `impl-dummy`。
- `FsError` 由实现方把底层 I/O 与格式错误映射到稳定枚举；`SharedFs` / `SharedRwFs` 使用
  `spin::Mutex`，调用方需保证与平台调度策略一致的访问边界。

## 调用链路

初始化与挂载：

```text
fs::init
  -> log_supported_fs()                 // 打印各 impl 声明的能力
  -> devfs::active_impl::refresh()      // 刷新 devfs 节点
  -> 探测根块设备并注入活动 impl（rootfs::set_active_fs_impl）
bring-up
  -> mount_default_root_rw()            // 挂载单一 ext4 RW 视图
```

路径操作：

```text
VFS 层请求
  -> pick_fs_impl(kind, mode)           // 按 FsKind + FsAccessMode 选 impl
  -> FsImpl / LocalFs / LocalRwFs / ReadWriteFs 方法
  -> FsResult 映射 Linux errno
```

devfs / procfs：

```text
/dev 节点访问
  -> devfs::lookup_block_device / lookup_character_device / default_root_block_path
/proc 访问
  -> procfs 以 FsImpl 注册进统一表
```

## 各实现功能

### fs-api / 文件系统 API

`fs-api/api-v0/src/lib.rs`：

- `FsError`：`NotMounted` / `NotFound` / `NotAFile` / `Exists` / `NotEmpty` / `Unsupported` /
  `Driver` / `Corrupt` / `Io` / `NoSpace` 等。
- `FsKind`：`Ext2` / `Ext3` / `Ext4` / `DevFs` / `RamFs` / `Other(&'static str)`。
- `FsAccessMode`：`ReadOnly` / `ReadWrite`；`FsCapability { kind, access }`。
- `FsImpl`：聚合注册面（`name` / `supported` / `supports`）；`LocalFs` / `LocalRwFs` /
  `ReadOnlyFs` / `ReadWriteFs` trait 与 `SharedFs` / `SharedRwFs` 共享句柄。
- 目录与元数据类型：`FsDirEntry` / `FsMetadata` / `FsNodeId` / `FsNodeType`。

### fs-devfs / 设备文件系统

`fs-devfs/devfs-impl/impl-kernel/src/lib.rs`：

- `KernelDevFsManager`（零大小类型，经静态 `DEVFS: Mutex<DevFsImpl>` 访问）。
- `refresh()`：从驱动设备表刷新节点；`list_nodes()`；`set_dt_unsupported_paths()`。
- `lookup_block_device(path)` / `lookup_character_device(path)`；`default_root_block_path()`。
- `KernelDevFsImpl`（`pub static IMPL`）实现 `FsImpl`，注册进统一表。

### fs-procfs / 进程信息伪文件系统

- `impl-kernel` 提供进程信息伪文件（`/proc`），以 `FsImpl` 注册进统一表；`impl-dummy` 占位。

### fs-rootfs / 根文件系统

`fs-rootfs/rootfs-impl/impl-kernel/src/lib.rs`：

- 全局状态：`ROOT_FS` / `ROOT_RW_FS`（共享句柄）、`ROOT_DEV_PATH`、`ACTIVE_FS_IMPL`、
  `MOUNT_GENERATION`。
- `set_active_fs_impl` / `active_fs_impl`；`mount_default_root` / `mount_default_root_rw` /
  `mount_root_rw_from_block_path` / `mount_aux_ro/rw_from_block_path`。
- `root_fs()` / `root_rw_fs()` / `current_root_device_path()` / `mount_generation()`。
- 职责边界：只维护“当前根卷”句柄与根块设备路径，具体 FS 种类由注入的 `FsImpl` 决定。

### fs-impl / 具体文件系统

- `impl-another-ext4`：默认 ext4 RW 后端，适配 vendored `another_ext4`（固定 4096 块、
  同步块设备 trait）；superblock magic `0xEF53`；带 lookup cache 与 negative cache。
- `impl-ext4-rs` / `impl-ext4`：可选 ext4 后端（回退 feature，互斥）。
- `impl-ramfs`：物理页 payload 后端 ramfs；tmpfs 由 VFS 策略层基于它创建挂载实例。
- `impl-devfs`：devfs 的 fs impl 适配。
- `impl-dummy`：无硬件占位。

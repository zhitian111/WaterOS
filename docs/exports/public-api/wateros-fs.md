# wateros-fs — 聚合层公共 API

## 用途

描述根内核与 `wateros-vfs` bridge **实际使用**的 `wateros-fs` 导出符号。

事实来源：`os/components/wateros-fs/src/lib.rs`；根 `os/Cargo.toml` 中 `fs = { package = "wateros-fs", ... }`。

## 顶层模块

```text
fs::api              // wateros-fs-api-v0 别名
fs::devfs            // 设备 FS 子系统
fs::procfs           // proc 伪 FS 子系统
fs::rootfs           // 根卷与辅助挂载
fs::init             // 子系统初始化（探测，不挂载）
fs::mount_default_root_rw
fs::root_rw_fs
fs::registered_fs_impls / pick_fs_impl / supported_fs_summary
```

可选直接依赖 impl crate（feature 条件）：

- `fs::impl_ext4_rs`（`impl-ext4-rs`）
- `fs::impl_ext4`（`impl-ext4`）

## 生命周期

| 函数 | 说明 |
|------|------|
| `init()` | 打印能力、刷新 devfs、probe 根块设备并 `set_active_fs_impl` |
| `mount_default_root_rw()` | bring-up 挂载默认根块设备为 RW |
| `root_rw_fs()` | 当前根 `SharedRwFs`，未挂载为 `None` |
| `mount_aux_ro_from_block_path` | 独立 RO 卷 |
| `mount_aux_rw_from_block_path` | 独立 RW 卷 |
| `test()` | API + procfs + 可选 ext4 自检 |

## `fs::api` 核心类型

| 类型 / trait | 说明 |
|--------------|------|
| `FsError` / `FsResult` | 统一错误 |
| `FsKind` / `FsAccessMode` / `FsCapability` | 能力与探测 |
| `FsMetadata` / `FsDirEntry` / `FsNodeType` | 元数据 |
| `ReadOnlyFs` | RO 根卷：mount、read、read_dir、read_symlink |
| `ReadWriteFs` | RW 根卷：写、mkdir、unlink、xattr、rename、symlink、mknod |
| `FsImpl` | 注册表项：name、supported、probe、mount_ro/rw 工厂 |
| `SharedFs` / `SharedRwFs` | 线程共享句柄 |
| `LocalFs` / `LocalRwFs` | 句柄上的便捷方法（Deref 到内部 trait 对象） |

## `fs::devfs::active_impl`

| 符号 | 说明 |
|------|------|
| `refresh()` | 从驱动刷新设备节点 |
| `lookup_block_device(path)` | 路径 → `SharedBlockDevice` |
| `default_root_block_path()` | 默认根块设备路径（如 `/dev/vda`） |
| `list_dev_nodes()` | 枚举 dev 节点 |

## `fs::procfs::active_impl`

| 符号 | 说明 |
|------|------|
| `view()` | `&'static impl ProcFsView` |
| `register_task_argv_lookup` / `register_task_exe_lookup` | proc 内容回调 |
| `register_mount_list_lookup` | `/proc/mounts` 行来源 |
| `IMPL` | `FsImpl` 静态实例 |

## `fs::rootfs::active_impl`

| 符号 | 说明 |
|------|------|
| `set_active_fs_impl` | 注入 probe 选中的 `FsImpl` |
| `mount_default_root_rw` | 挂载根卷 |
| `root_rw_fs` / `mount_generation` | 句柄与代次（页缓存失效用） |
| `mount_aux_*_from_block_path` | 辅助卷 |

## 注册表

| 函数 | 说明 |
|------|------|
| `registered_fs_impls()` | 静态 `FsImpl` 表（ext4-rs、ext4、devfs、procfs） |
| `pick_fs_impl(kind, mode)` | 按能力与模式选取 |
| `supported_fs_summary()` | 扁平化能力列表 |

## 未通过聚合层导出的内容

- `impl-ext4` / `impl-ext4-rs` 内部 ext4_rs / ext4plus 细节
- devfs/procfs impl-kernel 的格式化函数实现
- 块设备驱动 API（经 `driver_block_api_v0` 间接使用）

VFS bridge 与 bring-up 应使用 `fs::rootfs`、`fs::devfs`、`fs::procfs` 与 `fs::api` trait，避免 syscall 层直接依赖 `wateros-fs-impl-ext4-rs`。

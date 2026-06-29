# wateros-fs — 已实现功能快照

## 用途

记录 `wateros-fs` 一级组件当前已落地能力、feature 组合与已知缺口。事实来源：`os/components/wateros-fs/**` 源码与 `Cargo.toml`；根 `wateros` 直接依赖 `fs` crate。

## 子 crate 与职责

| 子 crate | 职责 | 状态 |
|----------|------|------|
| `wateros-fs`（聚合） | impl 注册表、`init`/`mount_default_root_rw`、devfs/procfs/rootfs 再导出 | 已实现 |
| `wateros-fs-api-v0` | `FsImpl`、`ReadOnlyFs`/`ReadWriteFs`、`SharedFs`/`SharedRwFs` | 已实现 |
| `fs-devfs` + api/impl | 块设备枚举、`/dev` 节点、默认根块路径 | 已实现 |
| `fs-procfs` + api/impl | `/proc` 伪文件（status、maps、mounts 等） | 已实现 |
| `fs-rootfs` + api/impl | 根卷句柄、辅助 RO/RW 挂载、挂载代次 | 已实现 |
| `fs-impl/impl-ext4-rs` | 基于 `ext4_rs` 的 ext4 RO/RW（**默认**） | 已实现 |
| `fs-impl/impl-ext4` | 基于 ext4plus 的旧路径（`impl-ext4` feature） | 已实现 |
| `fs-impl/impl-devfs` | devfs 的 `FsImpl` 注册面 | 已实现 |
| `fs-impl/impl-dummy` | API 层占位 | 已实现 |

## Feature 矩阵（聚合层）

| Feature | 效果 |
|---------|------|
| `api-v0` | 链接各子 crate API |
| `impl-devfs` | devfs 实现注册 |
| `impl-ext4-rs` | 默认 ext4 RW（`ext4_rs`） |
| `impl-ext4` | 可选 ext4plus 实现 |
| `default` | `api-v0` + `impl-devfs` + `impl-ext4-rs` |

## 已实现能力

- **启动序列**：`fs::init()` 刷新 devfs、探测根块设备、注入 `FsImpl`（**不**挂载）；`mount_default_root_rw()` 在 bring-up 挂载单一 RW 根卷。
- **ext4**：probe magic、RO/RW 挂载、整文件/区间读写、目录、symlink、hardlink、chmod/chown/xattr、truncate、rename、mknod 子集。
- **辅助卷**：`mount_aux_ro_from_block_path` / `mount_aux_rw_from_block_path` 独立句柄，不替换根卷。
- **devfs**：平台块设备刷新、lookup、`default_root_block_path`。
- **procfs**：按 tid 生成 status/maps/mounts 等；可注册 argv/exe/mount 列表回调。
- **共享句柄**：`Arc<Mutex<...>>` 型 `SharedFs` / `SharedRwFs`，`LocalFs`/`LocalRwFs` 薄包装。

## 与 wateros-vfs 的分工

`wateros-fs` 提供块 FS 与伪 FS **实现**；`wateros-vfs` 经 `impl-fs-bridge` 消费本组件 API，叠加挂载表、页缓存与 fd 语义。syscall 侧通常只依赖 `vfs`，不直接依赖 `fs-impl-*`。

## 缺口与后续

- `FsAsyncIo` 未实现，均为 `Unsupported`。
- `impl-ext4` 与 `impl-ext4-rs` 二选一由 feature 控制，勿同时依赖两套写路径。
- procfs 字段为 LTP/bring-up 子集，非完整 Linux `/proc`。
- NUMA、quota、journal 异常恢复等生产级语义未覆盖。
- `impl-dummy` rootfs/devfs 无真实设备。

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版导出（注释/inline 任务同步） |

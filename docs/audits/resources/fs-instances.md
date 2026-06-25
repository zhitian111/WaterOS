# 文件系统实例资源生命周期审计（fs-instances）

> **分组**：`fs-instances`  
> **覆盖资源**：#30 根卷 RO/RW 全局槽、#31 辅助挂载表项、#32 SharedFs / SharedRwFs、#33 ext4 磁盘 inode、#34 TmpFs inode/节点树、#35 DevFS 节点  
> **审计时间**：2026-06-25  
> **Baseline**：单核多线程；对照 Linux `mount`/`umount`/`ENOSPC`/`EBUSY`/`ENOMEM` 语义  
> **交叉参考**：`docs/audits/resource-inventory.md`、`docs/exports/features/wateros-fs.md`、`docs/audits/lock-inventory.md`（挂载表 `AUX_MOUNTS` 锁）

---

## 总览

| # | 资源 | 所属组件 | 账本稳定性 | 主要风险等级 |
|---|------|---------|-----------|-------------|
| 30 | 根卷 RO/RW 全局槽 | `fs-rootfs/impl-kernel` | **部分稳定** | 重挂载无显式清理；RO/RW 双槽语义分裂 |
| 31 | 辅助挂载表项 | `impl-fs-bridge/mount_table` | **部分稳定** | `device_minor` 耗尽 panic；无子挂载/EBUSY 检查 |
| 32 | SharedFs / SharedRwFs | `fs-api` + `impl-ext4-rs` + `mount_table` | **不可靠** | 同设备重复 RW 挂载；部分失败路径副作用 |
| 33 | ext4 磁盘 inode | `impl-ext4-rs` + `ext4_rs` | **部分稳定** | `ENOSPC` 映射不准；`rmdir` 依赖 vendor `unlink` |
| 34 | TmpFs inode / 节点 | `impl-fs-bridge/tmpfs` | **不可靠** | inode 单调递增；堆无上限 |
| 35 | DevFS 节点 | `devfs-impl-kernel` | **部分稳定** | `refresh` 全量重建；伪节点无设备绑定 |

---

## 资源 #30：根卷 RO/RW 全局槽

### 主要类型与位置

| 项 | 说明 |
|----|------|
| 结构体 | `static ROOT_FS: Mutex<Option<SharedFs>>`、`ROOT_RW_FS: Mutex<Option<SharedRwFs>>`、`ROOT_DEV_PATH`、`MOUNT_GENERATION: AtomicU64` |
| 文件 | `os/components/wateros-fs/fs-rootfs/rootfs-impl/impl-kernel/src/lib.rs` |
| API | `RootFsManager` trait（`fs-rootfs/rootfs-api/api-v0`） |

### 1. 分配入口

| 路径 | 函数 | 条件 |
|------|------|------|
| bring-up 主路径 | `mount_default_root_rw()` → `mount_root_rw_from_block_path()` | `fs::init` 已 `set_active_fs_impl`；devfs 有默认块设备 |
| 显式 RO 根挂载 | `mount_root_from_block_path()` / `mount_default_root()` | 写入 `ROOT_FS`（`set_root_fs`） |
| 显式 RW 根挂载 | `mount_root_rw_from_block_path()` | 写入 `ROOT_RW_FS` |
| 辅助别名复用 | `mount_aux_*_from_block_path` 检测 `Arc::ptr_eq(device, root_dev)` | 返回已有根句柄克隆，不新建 Ext4 |

**前置依赖**：`devfs::lookup_block_device`、`ACTIVE_FS_IMPL` 已注入、`FsImpl::mount_ro/rw`。

### 2. 回收入口

| 路径 | 函数 | 现状 |
|------|------|------|
| 显式清除 | `clear_root_fs()` | 清空 `ROOT_FS`、`ROOT_RW_FS`、`ROOT_DEV_PATH` |
| 隐式释放 | 覆盖槽位（新 `mount_root_rw` 赋值） | 旧 `Arc` 引用计数归零时 Drop |
| 生产调用 | **无** | 全仓库仅 trait 定义与 dummy 实现；**从未在 bring-up / syscall 中调用** |

根卷**无** `umount("/")` 路径；`unmount_aux_at` 拒绝 `mp == "/"`。

### 3. 生命周期状态机

```
[未注入 impl] ──set_active_fs_impl──► [impl 就绪，未挂载]
                                              │
                    mount_root_rw_from_block_path
                                              ▼
                                    [ROOT_RW_FS = Some(Arc)]
                                              │
              ┌───────────────────────────────┼───────────────────────────────┐
              │ 覆盖挂载（无 clear）            │ clear_root_fs（仅测试/文档）      │
              ▼                               ▼                               │
     [新 Arc 替换旧 Arc]                  [None / 未挂载] ◄────────────────────┘
```

**半初始化状态**：`mount_root_rw_from_block_path` 在 `imp.mount_rw` 成功后写槽位；`mount_rw` 内部 `Ext4::open` 失败时返回 `Err`，槽位不变。

**语义分裂**：bring-up 仅设置 `ROOT_RW_FS`，`ROOT_FS` 恒为 `None`；`list_proc_mount_lines` 以 `root_rw_fs().is_some()` 判断根挂载行。

### 4. 账本稳定性

- **Arc 引用计数**：稳定；克隆句柄与槽位共享底层 `Ext4RsFs`。
- **重挂载**：不调用 `clear_root_fs`，直接覆盖；旧实例在无其他 `Arc` 时正确释放。
- **RO/RW 双槽**：`clear_root_fs` 同时清两者，但正常运行期只使用 RW 槽；`mount_root_from_block_path`（RO）与 `mount_root_rw` 可形成双槽并存，VFS `root_rw()` 只读 RW 槽。
- **风险**：无 double-free；重挂载无泄漏（Arc 语义）。

### 5. 耗尽处理

| 场景 | 行为 |
|------|------|
| 槽位容量 | 各 **1** 槽（硬编码） |
| 重复根挂载 | 静默覆盖，返回 `Ok` |
| 无块设备 | `FsError::NotMounted` / init warn |
| 无 impl | `FsError::Unsupported` |

### 6. 跨资源耦合

- `mount_generation()` 供页缓存键；根挂载与辅助挂载均通过 `bump_mount_generation` 失效缓存。
- VFS `root_rw()` 直接读 `ROOT_RW_FS`；页缓存 `FsPageIo` 写路径对根卷走 `root_rw()`。
- devfs `default_root_block_path()` 决定默认设备。

### 潜在问题

| 严重度 | 问题 | 说明 |
|--------|------|------|
| P2 | `clear_root_fs` 无生产调用方 | 文档称用于「卸载或错误恢复」，实际无路径；依赖 Arc 覆盖 |
| P2 | RO/RW 双槽并存语义未文档化 | RO 根 + RW 根理论可共存，VFS 仅见 RW |
| P3 | 根卷不可卸载 | 与 Linux 不同；测例依赖常驻根卷，可接受 |

### 收敛建议

- 重挂载根卷前显式 `clear_root_fs` 或文档约定「覆盖即替换」。
- 统一 bring-up 只使用 RW 槽，废弃 `ROOT_FS` 或明确双视图用途。

### 修复任务草案

1. **标题**：根卷重挂载前显式释放旧句柄  
   **文件**：`fs-rootfs/rootfs-impl/impl-kernel/src/lib.rs`  
   **验收**：`mount_root_rw_from_block_path` 在赋值前 drop 旧槽位并打 warn 日志；`mount_generation` 仍递增。

---

## 资源 #31：辅助挂载表项

### 主要类型与位置

| 项 | 说明 |
|----|------|
| 结构体 | `AUX_MOUNTS: Mutex<Vec<MountEntry>>`、`DEVICE_IDS`、`NEXT_DEVICE_MINOR`、`NEXT_MOUNT_ID` |
| `MountEntry` | `mount_point`, `fs: AuxMount`, `identity: MountIdentity`, `readonly`, `fstype` |
| 文件 | `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/mount_table.rs` |

### 1. 分配入口

| 路径 | 函数 | syscall |
|------|------|---------|
| ext4 块设备 | `mount_ext4_block_at` → `mount_aux_at_ro/rw` → `mount_aux_common` | `mount(2)` ext4 分支 |
| tmpfs | `mount_tmpfs_at` | `mount(2)` tmpfs |
| cgroup | `mount_cgroup_at` | `mount(2)` cgroup/cgroup2 |
| procfs 伪挂载 | `mount_aux_proc_at` | `mount(2)` proc |
| 自检 | `mount_table_self_test` | 内部 |

**前置**：`assert_mount_point_directory`（挂载点须为已存在目录）；`mount_point != "/"`；同路径不重复（`Exists`）。

### 2. 回收入口

| 路径 | 函数 |
|------|------|
| 用户态卸载 | `unmount_aux_at` ← `vfs::unmount_at` ← `sys_umount2` |
| 隐式 | `table.remove` 后 `MountEntry` Drop → `AuxMount` Drop → `SharedFs`/`SharedRwFs` Arc 释放 |

**未实现**：`DEVICE_IDS` / `device_minor` 回收；`mount_id` 单调递增不复用。

### 3. 生命周期状态机

```
[表空] ──mount_aux_common──► [MountEntry 入 Vec]
                                    │
                    ┌───────────────┼───────────────┐
                    │ remount_aux_readonly          │ unmount_aux_at
                    ▼                               ▼
            [readonly=true]                    [表项移除，Arc Drop]
```

**半初始化**：

- `mount_tmpfs_at`：先 `Arc::new(TmpFs)`，再 `mount_aux_common`；后者失败时 `Arc` 随栈 Drop（**无表项泄漏**）。
- `mount_ext4_block_at`：先 `mount_aux_*_from_block_path`（**已创建 Ext4 + bump mount_generation**），再 `mount_aux_common`；后者失败时 Ext4 `Arc` Drop，但 **mount_generation 已被提前 bump**（副作用残留）。

### 4. 账本稳定性

- 表项与 `Arc` 成对：卸载时移除表项即释放 FS 实例（无表项泄漏）。
- **最长前缀路由**：`longest_aux_mount` 克隆 `Arc` 增加引用；卸载后若 fd 仍持有旧路径，路由行为改变（见 #32 交叉项）。
- **子挂载**：卸载父挂载点**不**检查是否存在 `mount_point + "/"` 前缀的子挂载（与 Linux `EBUSY` 不符）。
- **ID 账本**：`device_minor_for` 耗尽时 `.expect("VFS device id exhausted")` → **panic**。

### 5. 耗尽处理

| 场景 | 行为 | Linux 差距 |
|------|------|-----------|
| 重复挂载点 | `VfsError::Exists` → `EBUSY` | 一致 |
| `device_minor` ≥ u32::MAX | **panic** | 应返回错误 |
| 辅助卷数量 | 无硬上限（`Vec` 增长） | 无 `EMFILE` 类限制 |
| umount 不存在 | `EINVAL` | 基本一致 |
| umount 有打开文件/子挂载 | **仍成功** | Linux `EBUSY` |

### 6. 跨资源耦合

- 挂载/卸载调用 `bump_mount_generation_after_cache_flush` → `reset_file_page_cache` 逻辑链。
- `mount_ext4_block_at` 依赖 `fs::mount_aux_*_from_block_path`（rootfs 层）。
- procfs 挂载依赖根卷上 `ensure_proc_mount_point`（`mkdir /proc`）。

### 潜在问题

| 严重度 | 问题 |
|--------|------|
| **P0** | `device_minor` 耗尽 **panic**（长跑/频繁 mount 可触发内核崩溃） |
| **P1** | `umount` 不检查子挂载（`AUX_MOUNTS` 中存在更长 `mount_point` 前缀） |
| **P1** | `umount` 不检查挂载点是否 busy（打开 fd、页缓存 `open_refs`） |
| **P2** | `DEVICE_IDS` 只增不减 |
| **P2** | `mount_ext4` 失败时 `mount_generation` 被 `mount_aux_*_from_block_path` 提前 bump |

### 收敛建议

- `device_minor_for` 改为返回 `VfsError::Io` + warn（含 `used` 计数），禁止 panic。
- `unmount_aux_at` 遍历表检查子挂载前缀，存在则 `EBUSY`。
- 将 `mount_generation` bump 移入 `mount_aux_common` 成功路径之后。

### 修复任务草案

1. **标题**：`device_minor` 耗尽安全失败  
   **文件**：`mount_table.rs` `device_minor_for`  
   **验收**：`NEXT_DEVICE_MINOR` 溢出时返回 `Err`，syscall 映射 `ENOMEM` 或 `EINVAL`，无 panic。

2. **标题**：umount 子挂载 busy 检查  
   **文件**：`mount_table.rs` `unmount_aux_at`  
   **验收**：存在 `ent.mount_point.starts_with(mp + "/")` 时返回 `VfsError::Busy` → `EBUSY`。

---

## 资源 #32：SharedFs / SharedRwFs 实例

### 主要类型与位置

| 项 | 说明 |
|----|------|
| 类型别名 | `SharedFs = Arc<Mutex<LocalFs>>`、`SharedRwFs = Arc<Mutex<LocalRwFs>>` |
| 包装 | `LocalFs(Box<dyn ReadOnlyFs>)`、`LocalRwFs(Box<dyn ReadWriteFs>)` |
| 工厂 | `Ext4RsImpl::mount_ro/rw`（每次 `Ext4RsFs::new()` + `Ext4::open`） |
| 文件 | `fs-api/api-v0/src/lib.rs`、`fs-impl/impl-ext4-rs/src/lib.rs` |

### 1. 分配入口

```
FsImpl::mount_ro/rw(device)
  → Ext4RsFs::new()
  → ReadOnlyFs::mount / ReadWriteFs::mount_rw
  → Ext4::open(Arc<BlockDevAdapter>)
  → Arc::new(Mutex::new(LocalFs/LocalRwFs::new(Box::new(fs))))
```

tmpfs/cgroup：`Arc::new(Mutex::new(LocalRwFs::new(Box::new(TmpFs::new()))))`。

### 2. 回收入口

- `AUX_MOUNTS` 表项移除 → `AuxMount` Drop。
- 根槽位覆盖 / `clear_root_fs`。
- **无** `FsImpl::unmount` API；完全依赖 `Arc` 最后一个引用释放。

`Ext4RsFs` Drop 时 `Option<Ext4>` 释放；**无**显式 flush/sync 卸载钩子。

### 3. 生命周期状态机

```
[未打开] ──mount_rw──► [Ext4 已打开，is_mounted=true]
                              │
              ┌───────────────┼───────────────────────┐
              │ Arc::clone（路由/fd/表项）              │ 最后一个 Arc Drop
              ▼                                       ▼
        [共享使用中]                              [Ext4 内存态释放]
```

### 4. 账本稳定性

| 路径 | 结论 |
|------|------|
| 单设备单实例（根卷 + 别名复用） | **稳定** |
| 同块设备两次独立 `mount_rw`（不同挂载点，非根别名） | **不可靠 — 双 Ext4 实例写同一设备** |
| umount 后 fd 仍通过绝对路径 I/O | **不可靠 — 路由回落根卷或 NotFound** |
| `Arc` 引用计数 | 稳定 |

**同设备重复挂载验证**：`mount_aux_*_from_block_path` 仅对**当前根设备**做 `Arc::ptr_eq` 复用；两辅助挂载点挂同一 `/dev/vblkN`（非根）会各调 `imp.mount_rw`，产生两个独立 `Ext4` 对象。

### 5. 耗尽处理

| 场景 | 行为 |
|------|------|
| `Ext4::open` 失败 | `mount_rw` 返回 `Err`，无 `Arc` 泄漏 |
| 堆分配 `Arc`/Ext4 | 失败时向上传播（依赖全局分配器） |
| 实例数量上限 | **无** |

### 6. 跨资源耦合

- 块设备 `SharedBlockDevice`（driver 注册表 #36）。
- 页缓存经路径 + `mount_gen` 访问，不持有 `SharedRwFs`。
- `PagedFileHandle` 关闭时 `release_open_ref`；umount **不**遍历 fd 表。

### 潜在问题

| 严重度 | 问题 |
|--------|------|
| **P0** | 同块设备多次 RW 挂载 → 独立缓存并发写 → **磁盘一致性破坏** |
| **P0** | umount 后已打开 fd 继续 I/O → 路径路由到错误 FS 或静默失败 |
| **P1** | umount 无 sync/fsync 刷盘钩子 |
| **P2** | `mount_ext4` 两步分配导致 generation 提前 bump |

### 收敛建议

- 维护全局「已挂载块设备 → `Weak<SharedRwFs>`」表；重复 `mount_rw` 同 `Arc::ptr_eq(device)` 时复用或返回 `EBUSY`。
- `unmount_aux_at` 前检查 fd 表 / 页缓存 `open_refs` 是否含该挂载点前缀路径。
- umount 前对 RW 卷做 best-effort flush（或文档明确不保证）。

### 修复任务草案

1. **标题**：块设备挂载去重注册表  
   **文件**：`fs-rootfs/impl-kernel` 或 `mount_table.rs`  
   **验收**：同一 `SharedBlockDevice` 第二次 RW 挂载返回错误或返回已有 `Arc`；LTP 双挂同设备测例行为可预期。

2. **标题**：umount busy 检测（fd + 页缓存 open_refs）  
   **文件**：`mount_table.rs`、`impl-fd-session`  
   **验收**：挂载点下存在未关闭 fd 时 `umount2` 返回 `EBUSY`。

---

## 资源 #33：ext4 磁盘 inode

### 主要类型与位置

| 项 | 说明 |
|----|------|
| 底层 | `ext4_rs::Ext4`（`ialloc`、`fuse_*`） |
| 适配层 | `impl-ext4-rs/src/lib.rs` — `create_regular`、`create_directory`、`unlink`、`rmdir` |
| 释放 | `fs.ialloc_free_inode(inode_num, is_dir)` |

### 1. 分配入口

| 操作 | 函数链 |
|------|--------|
| 创建文件 | `write_regular_file` / `create_regular` → `fuse_mknod` |
| 创建目录 | `mkdir` → `create_directory` → `fs.create` |
| 符号链接 | `symlink` → `fuse_symlink` + `write_all` |
| 硬链接 | `hardlink` → `fs.link`（不新分配 inode） |
| 设备节点 | `mknod` → `fuse_mknod` |

### 2. 回收入口

| 操作 | 函数链 | inode 释放 |
|------|--------|-----------|
| 删文件 | `unlink` | `nlink <= 1` 时 `truncate` + `ialloc_free_inode` |
| 删目录 | `rmdir` → `fuse_rmdir` | vendor `unlink` → `ialloc_free_inode` |
| 硬链接减链 | `unlink`（`nlink > 1`） | 仅 `links_count--`，不 free |

### 3. 生命周期状态机

```
[空闲位图] ──ialloc──► [inode 已分配，目录项存在]
                              │
              ┌───────────────┼───────────────┐
              │ hardlink +1                   │ unlink/rmdir
              ▼                               ▼
        [nlink > 1]                      [nlink 减至 0]
                                              │
                                              ▼
                                    [ialloc_free_inode → 位图回收]
```

**半初始化**：`create_directory` 中 `fs.create` 成功后 `write_back_inode`；若中间失败由 ext4_rs 内部事务处理（依赖 vendor）。

### 4. 账本稳定性

- **unlink 普通文件**：适配层显式 free，与 nlink 一致 → **稳定**。
- **rmdir**：`fuse_rmdir` 调用 `unlink`（vendor），`ialloc_free_inode` 执行 → **稳定**（vendor 内 TODO 注释已过时）。
- **rename**：同目录内 `dir_add_entry` + `dir_remove_entry`，不增减 inode → **稳定**。
- **跨目录 rename**：返回 `Unsupported`，无半成品。
- **位图与实际**：依赖 ext4_rs beta 完整性；无 journal 时掉电风险在功能文档已声明。

### 5. 耗尽处理

| 场景 | ext4_rs | 映射到用户态 |
|------|---------|-------------|
| inode 位图满 | `Errno::ENOSPC` | `FsError::Io`（**非** `ENOSPC`） |
| 块耗尽 | `ENOSPC` | `FsError::Io` |
| `EINVAL` 等 | — | `InvalidPath` / `Io` |

syscall 层进一步映射为通用 I/O 错误，**与 Linux `ENOSPC` 不符**。

### 6. 跨资源耦合

- 块分配与块设备写缓存（#19）。
- 页缓存写回触发 `write_range` / `zero_extend_file`（空洞扩展消耗块）。
- 根卷与辅助 ext4 卷各自持有独立 `Ext4` 实例（见 #32）。

### 潜在问题

| 严重度 | 问题 |
|--------|------|
| P1 | inode/块耗尽时错误码为 `EIO` 类而非 `ENOSPC` |
| P2 | ext4plus beta / 无 journal；资源账本与掉电恢复不在本审计范围 |
| P3 | `create_regular` 中 `fuse_mknod` 与 `fuse_lookup` 非原子（vendor 层） |

### 收敛建议

- `map_ext4_rs` 将 `ENOSPC` 映射为独立 `FsError` 或直传 errno；syscall 返回 `-ENOSPC`。
- 同设备单 Ext4 实例（修复 #32 P0）。

### 修复任务草案

1. **标题**：ext4 `ENOSPC` 错误码透传  
   **文件**：`impl-ext4-rs/src/lib.rs`、`vfs_util` errno 映射  
   **验收**：位图满时 `creat`/`mkdir` 返回 `-ENOSPC`。

---

## 资源 #34：TmpFs inode / 节点树

### 主要类型与位置

| 项 | 说明 |
|----|------|
| 结构体 | `TmpFs { root: TmpNode, next_inode: u64, mounted: bool }` |
| 节点 | `TmpNode::File/Dir/Symlink`（含 `data: Vec<u8>`、`children: BTreeMap`） |
| 文件 | `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/tmpfs.rs` |

### 1. 分配入口

| 操作 | inode 分配 |
|------|-----------|
| `mkdir` / `symlink` / `write_regular_file`（新建） | `alloc_inode()`：`next_inode++` |
| `write_regular_file`（覆盖已存在） | **新 inode**（先 `remove_leaf` 再插入新节点） |
| 挂载 | `TmpFs::new()`：根 inode=1，`next_inode=2` |

### 2. 回收入口

| 操作 | 节点内存 | inode 编号 |
|------|---------|-----------|
| `unlink` | `children.remove` → `TmpNode` Drop | **不回收编号** |
| `rmdir` | 空目录 remove | **不回收编号** |
| `unmount_aux_at` | 整棵 `TmpFs` 随 `Arc` Drop | 全部释放 |
| `rename` | 节点所有权转移 | inode 号随行 |

### 3. 生命周期状态机

```
[next_inode=N] ──alloc_inode──► [节点入树，inode=N]
                                      │
                                      │ unlink/rmdir
                                      ▼
                               [节点出树，N 永不复用]
```

### 4. 账本稳定性

- **节点树**：分配/删除成对，无悬空 `BTreeMap` 引用 → **稳定**。
- **inode 编号**：单调递增，**永不回收到空闲池** → 长跑测试 inode 号膨胀（功能正确，账本虚高）。
- **`write_regular_file` 覆盖**：每次新 inode；旧节点已 remove，无泄漏，但 `next_inode` 额外 +1。
- **堆内存**：文件数据 `Vec`、目录 `BTreeMap` 无上限 → 依赖内核堆 #6。

### 5. 耗尽处理

| 场景 | 行为 |
|------|------|
| `next_inode` 溢出 | `u64`  practically 不溢出 |
| 堆分配失败 | `Vec`/`BTreeMap` 增长失败 → 分配器 OOM（可能 panic 或 `Err`） |
| 文件大小 | **无** tmpfs 配额 |
| 节点数 | **无** 上限 |

### 6. 跨资源耦合

- 仅通过 `AUX_MOUNTS` 持有；cgroup 挂载复用 `TmpFs` + 预置文件。
- umount 释放全部 tmpfs 内存；无逐文件 fd 跟踪。

### 潜在问题

| 严重度 | 问题 |
|--------|------|
| **P1** | tmpfs 无内存/节点硬上限 → 长跑 LTP **内核堆静默耗尽** |
| P2 | inode 编号不回收 → `/proc` mount stat 类测例可能介意大 inode 号 |
| P3 | `mounted` 标志 umount 时不置 false（仅整实例 Drop，无功能影响） |

### 收敛建议

- 增加 `TMPFS_MAX_BYTES` / `TMPFS_MAX_NODES`；超限时 warn + `ENOSPC`。
- 可选：inode 空闲链表复用（低优先级）。

### 修复任务草案

1. **标题**：tmpfs 堆用量硬上限  
   **文件**：`tmpfs.rs`  
   **验收**：累计分配超过配置阈值时 `mkdir`/`write` 返回错误并打 warn（含 `used/limit`）。

---

## 资源 #35：DevFS 节点

### 主要类型与位置

| 项 | 说明 |
|----|------|
| 结构体 | `DevFsImpl { nodes, block_bindings, character_bindings, dt_unsupported_paths }` |
| 静态 | `static DEVFS: Mutex<DevFsImpl>` |
| 文件 | `os/components/wateros-fs/fs-devfs/devfs-impl/impl-kernel/src/lib.rs` |

### 1. 分配入口

| 路径 | 函数 |
|------|------|
| 启动 / 显式刷新 | `refresh()` — 扫描 `block_device_count` / `character_device_count` 重建 |
| 手动注册 | `register_block_device` / `register_character_device` |
| DT 占位 | `set_dt_unsupported_paths` → 下次 `refresh` 合并 |
| 伪字符节点 | `refresh` 末尾注入 `/dev/null` 等（**无** `character_bindings`） |

### 2. 回收入口

| 路径 | 行为 |
|------|------|
| `refresh()` | **全量** `nodes.clear()` + `block_bindings.clear()` + `character_bindings.clear()` 后重建 |
| 设备注销 | **无** unregister API（与 driver #36–38 一致） |
| 单路径覆盖 | `register_*` 可更新已有路径的 `Arc` 绑定 |

### 3. 生命周期状态机

```
[空 DevFsImpl] ──refresh──► [节点 + 绑定快照]
                                  │
                    ┌─────────────┼─────────────┐
                    │ register_*（增量）         │ refresh（全量重建）
                    ▼                           ▼
              [单路径更新]                  [旧 Vec 整体 Drop]
```

**半初始化**：`refresh` 先采集 driver 快照再清空；清空后重建前持锁，无对外暴露空表窗口。

### 4. 账本稳定性

- **节点与绑定**：成对入 `nodes` + `block_bindings`/`character_bindings`；`refresh` 原子替换 → **稳定**。
- **伪节点**（`/dev/zero` 等）：仅在 `nodes` 列表，**`lookup_character_device` 失败** → 打开走 VFS 内置路径，与 devfs 账本分离。
- **`refresh` 与活跃挂载**：不 invalidate 已打开块设备 `Arc`（克隆自 driver）；旧 `block_bindings` Drop 不影响 driver 槽位。
- **风险**：运行期 `refresh` 若与 `lookup_block_device` 并发（同锁序列化）无 UAF；已克隆的 `SharedBlockDevice` 仍有效。

### 5. 耗尽处理

| 场景 | 行为 |
|------|------|
| 节点数量 | 随设备数线性增长，**无硬上限** |
| 重复路径 | `push_block_alias` 去重 |
| `FsImpl::mount_ro` | 恒 `Unsupported`（devfs 非块卷） |

### 6. 跨资源耦合

- 依赖 driver 块/字符设备注册表（#36–37）。
- `default_root_block_path` → 根卷挂载（#30）。
- `mount_aux_*_from_block_path` → `lookup_block_device`。

### 潜在问题

| 严重度 | 问题 |
|--------|------|
| P2 | 伪字符设备节点无绑定，`lookup` 失败靠 VFS 兜底，账本双源 |
| P3 | `refresh` 运行期调用会短暂持锁重建；设备数极大时延迟 |
| P3 | 无 unregister；热插拔场景节点陈旧直到下次 refresh |

### 收敛建议

- 文档明确伪节点与 `character_bindings` 分工。
- 设备热插拔时由 driver 层触发 `devfs::refresh` 并打日志。

### 修复任务草案

1. **标题**：伪 dev 节点与 lookup 表一致性  
   **文件**：`devfs-impl-kernel/src/lib.rs`  
   **验收**：`/dev/null` 等要么入 `character_bindings`，要么文档 + 断言仅走 VFS builtin 路径。

---

## 跨资源生命周期钩子（本分组）

| 事件 | 涉及资源 | 入口 | 缺口 |
|------|---------|------|------|
| `fs::init` | #35 devfs、#30 impl 注入 | `wateros-fs/src/lib.rs::init` | 不挂载根卷 |
| bring-up | #30 ROOT_RW_FS、#33 Ext4 | `mount_default_root_rw` | 不设置 ROOT_FS |
| `mount(2)` | #31 #32 #34 | `sys/mount.rs` → vfs bridge | 见上 P0/P1 |
| `umount2(2)` | #31 #32 | `sys/umount2.rs` → `unmount_aux_at` | 无 EBUSY |
| 页缓存代次 | #30 generation | `bump_mount_generation_after_cache_flush` | 部分失败路径误 bump |
| `close(fd)` | 间接 #32 路径 | 页缓存 `release_open_ref` | umount 不等待 |
| 进程 exit | fd 批量关闭 | `drop_task_fd_table` | 不触发 umount |

---

## 汇总：潜在问题清单

| ID | 严重度 | 资源 | 类型 | 摘要 |
|----|--------|------|------|------|
| FI-01 | **P0** | #32 | 磁盘损坏 | 同块设备独立多次 RW 挂载，双 Ext4 实例并发写 |
| FI-02 | **P0** | #31/#32 | UAF 语义 | umount 不检查 busy fd，卸载后 I/O 路由错误 |
| FI-03 | **P0** | #31 | 卡死 | `device_minor` 耗尽 panic |
| FI-04 | P1 | #31 | 语义不符 | umount 父挂载点不检查子挂载 |
| FI-05 | P1 | #34 | 静默耗尽 | tmpfs 无堆/节点上限 |
| FI-06 | P1 | #33 | 错误码 | ext4 `ENOSPC` 映射为 `Io` |
| FI-07 | P2 | #31/#32 | 副作用 | `mount_ext4` 表项失败前已 bump mount_generation |
| FI-08 | P2 | #30 | 文档/API | `clear_root_fs` 无调用方 |
| FI-09 | P2 | #35 | 账本双源 | 伪 dev 节点无 character 绑定 |

---

## 修复任务队列（草案，供文档 C 合并）

| 优先级 | 标题 | 文件 | 验收标准 |
|--------|------|------|---------|
| P0 | 块设备 RW 挂载去重 | `mount_table.rs` / `rootfs-impl-kernel` | 同 `SharedBlockDevice` 不创建第二 Ext4 实例 |
| P0 | umount busy 检测 | `mount_table.rs` + fd 表 | 有 open fd 或子挂载时 `-EBUSY` |
| P0 | device_minor 耗尽安全失败 | `mount_table.rs` | 无 panic，返回明确错误 |
| P1 | tmpfs 内存上限 | `tmpfs.rs` | 超限 warn + `ENOSPC` |
| P1 | ext4 ENOSPC 透传 | `impl-ext4-rs` + syscall errno | `creat`/`mkdir` 位图满 → `-ENOSPC` |
| P2 | mount_generation bump 顺序 | `mount_ext4_block_at` / `mount_aux_*` | 仅表项插入成功后 bump |
| P2 | 根卷重挂载显式 clear | `rootfs-impl-kernel` | 覆盖前 warn + drop 旧槽位 |

---

## 账本稳定性总结

| 资源 | 结论 | 一句话 |
|------|------|--------|
| #30 根卷槽 | 部分稳定 | Arc 正确，但双槽语义与无卸载 API |
| #31 挂载表 | 部分稳定 | 表项与 Arc 成对，ID 单调，umount 过松 |
| #32 SharedFs | **不可靠** | 同设备多实例 + umount 无 busy 检查 |
| #33 ext4 inode | 部分稳定 | 分配/释放链路完整，错误码与 beta 风险 |
| #34 tmpfs | **不可靠** | 节点可回收，inode/堆无上限 |
| #35 devfs | 部分稳定 | refresh 全量替换安全，伪节点双源 |

---

*本文件为 `fs-instances` subagent 单资源审计产出；主 agent 合并入 `docs/audits/resource-issues.md`、`resource-lifecycle.md`、`resource-fix-queue.md`。*

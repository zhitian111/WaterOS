# 锁机制审计：MountTable + RootFsGlobals

> 审计日期：2026-06-25（复核源码，无 lock 相关代码修复）  
> 审计范围：清单 #10（`AUX_MOUNTS` / `DEVICE_IDS`）与 #11（`ROOT_FS` 等 4 个静态 Mutex）  
> Baseline：单核多线程；`spin::Mutex` 为不可递归自旋锁  
> 关联调用方：`impl-fs-bridge/src/lib.rs`、`syscall mount/umount2`、页缓存 `paged_handle`

---

## 1. 数据结构概览

| 名称 | 文件 | 锁类型 | 保护内容 |
|------|------|--------|----------|
| `AUX_MOUNTS` | `mount_table.rs:31` | `spin::Mutex<Vec<MountEntry>>` | 辅助卷挂载表（最长前缀路由） |
| `DEVICE_IDS` | `mount_table.rs:32` | `spin::Mutex<Vec<(String, u32)>>` | 设备路径 → minor 号映射 |
| `NEXT_DEVICE_MINOR` / `NEXT_MOUNT_ID` | 同上 | `AtomicU64`（Relaxed） | minor / mount_id 分配 |
| `ROOT_FS` | `rootfs-impl/impl-kernel/src/lib.rs:20` | `spin::Mutex<Option<SharedFs>>` | 根卷 RO 句柄 |
| `ROOT_RW_FS` | 同上 `:21` | `spin::Mutex<Option<SharedRwFs>>` | 根卷 RW 句柄 |
| `ROOT_DEV_PATH` | 同上 `:22` | `spin::Mutex<Option<String>>` | 根块设备路径 |
| `ACTIVE_FS_IMPL` | 同上 `:25` | `spin::Mutex<Option<&'static dyn FsImpl>>` | 活动 FS 实现指针 |
| `MOUNT_GENERATION` | 同上 `:15` | `AtomicU64`（Acquire/Release） | 挂载代次（页缓存失效） |

**Per-FS 实例锁**（本组外围，审计交叉引用）：`SharedFs` / `SharedRwFs` = `Arc<spin::Mutex<LocalFs>>`，在 `resolve_route` **释放** `AUX_MOUNTS` 后才加锁。

---

## 2. 全部 lock / unlock 调用点

### 2.1 `AUX_MOUNTS`

| 函数 | 行 | 持锁区间 | 备注 |
|------|-----|----------|------|
| `longest_aux_mount` | 100 | 遍历 + `clone_mount()`（仅 `Arc` clone） | 热路径，每次 `resolve_route` |
| `mount_aux_common` | 166–170 | 重复挂载点检查 | **检查后释锁** |
| `mount_aux_common` | 175–181 | `lock().push(MountEntry)` | 与检查非原子（TOCTOU，MR-03） |
| `mount_statfs_magic` | 245 | 只读遍历 | |
| `remount_aux_readonly` | 269–279 | 修改 `readonly` + `bump_mount_generation` | 不回调 VFS |
| `is_proc_mounted_at` | 290–293 | 只读 `any` | |
| `list_proc_mount_lines` | 320 | 遍历辅助卷 | 先调 `root_rw_fs()` 再锁表 |
| `unmount_aux_at` | 335–342 | `remove` + `bump_mount_generation` | |
| `mount_table_self_test` | 352, 363 | 测试计数 | |

### 2.2 `DEVICE_IDS`

| 函数 | 行 | 持锁区间 |
|------|-----|----------|
| `device_minor_for` | 44–51 | 查找或分配 minor 并 `push` |

调用链：`new_mount_identity`（挂载时）、`root_identity`（根路径 `resolve_route` 热路径）。

### 2.3 RootFsGlobals（`ROOT_*` / `ACTIVE_FS_IMPL`）

| 函数 | 锁 | 行 | 持锁区间 |
|------|-----|-----|----------|
| `set_root_fs` | `ROOT_FS` | 29 | 单次赋值 |
| `root_fs` | `ROOT_FS` | 33 | clone `Arc` |
| `clear_root_fs` | `ROOT_FS` → `ROOT_RW_FS` → `ROOT_DEV_PATH` | 36–38 | 三次独立加锁，固定顺序 |
| `mount_root_from_block_path` | `ACTIVE_FS_IMPL`（极短）→ `ROOT_FS` → `ROOT_DEV_PATH` | 44–49 | `mount_ro` 在 `ACTIVE_FS_IMPL` 释锁后 |
| `current_root_device_path` | `ROOT_DEV_PATH` | 54 | clone |
| `set_active_fs_impl` | `ACTIVE_FS_IMPL` | 59 | 启动期 |
| `active_fs_impl` | `ACTIVE_FS_IMPL` | 63 | 拷贝指针后释锁 |
| `mount_root_rw_from_block_path` | `ACTIVE_FS_IMPL`（极短）→ `ROOT_RW_FS` → `ROOT_DEV_PATH` | 86–92 | |
| `root_rw_fs` | `ROOT_RW_FS` | 105 | clone `Arc` |
| `mount_aux_ro_from_block_path` | `ROOT_DEV_PATH`、`ROOT_FS`（别名复用路径）→ `ACTIVE_FS_IMPL`（极短） | 127–145 | 分次短持锁 |
| `mount_aux_rw_from_block_path` | 同上，用 `ROOT_RW_FS` | 153–172 | |

`MOUNT_GENERATION`：`fetch_add` / `load`，无 Mutex。

---

## 3. 锁顺序与嵌套持锁分析

### 3.1 观测到的全局锁顺序

```
（无文档化全局顺序；以下为代码实际顺序）

root_identity:     ROOT_DEV_PATH → DEVICE_IDS
clear_root_fs:     ROOT_FS → ROOT_RW_FS → ROOT_DEV_PATH
mount_aux_common:  AUX_MOUNTS(检查) → [释锁] → VFS/Per-FS → AUX_MOUNTS(push)
resolve_route:     AUX_MOUNTS → [释锁] → (根) ROOT_DEV_PATH → DEVICE_IDS → [释锁] → 调用方 Per-FS
list_proc_mount_lines: ROOT_RW_FS → [释锁] → AUX_MOUNTS
```

**跨组嵌套**：未发现同一线程同时持有 `AUX_MOUNTS` 与 `ROOT_*` / `DEVICE_IDS` 的代码路径。`mount_aux_common` 在调用 `assert_mount_point_directory`（会走完整 VFS/`resolve_route`）前后分段持锁，避免了 `AUX_MOUNTS` 自锁死。

**与 Per-FS / 页缓存**：VFS 路径统一为 `resolve_route`（全局表短持锁）→ `root_rw()?.lock()` 或 `fs.lock()`。与 `paged_handle.rs` 声明的页缓存锁序（先 cache 后 ext4）一致：挂载表锁不会延续到 ext4 持锁区间。

### 3.2 mount / umount 与 VFS 路径嵌套

#### `mount_aux_common` 完整链

```
mount_aux_common
├─ AUX_MOUNTS.lock()          // 重复点检查
├─ drop
├─ assert_mount_point_directory(mp)
│   └─ FsBridge::metadata(path)
│       └─ resolve_route
│           ├─ longest_aux_mount → AUX_MOUNTS.lock() → drop
│           └─ (根路径) root_identity → ROOT_DEV_PATH → DEVICE_IDS
│       └─ root_rw()?.lock() → ext4 metadata     // 可能块 I/O
├─ AUX_MOUNTS.lock()          // push
├─ drop
└─ bump_mount_generation()
```

#### `mount_ext4_block_at`（syscall `mount` ext4 路径）

```
mount_aux_{ro,rw}_from_block_path   // ROOT_* 短持锁 + ACTIVE_FS_IMPL 短持锁 + 块设备 mount（无全局表锁）
  → bump_mount_generation (×1)
mount_aux_at_{ro,rw}
  → mount_aux_common
  → bump_mount_generation (×2，同一次 ext4 挂载共 bump 两次)
```

#### `sys_umount2`

```
resolve_path_at → vfs::unmount_at → unmount_aux_at
  → AUX_MOUNTS.lock() → remove → bump_mount_generation
```

挂载/卸载路径**不会**在持 `AUX_MOUNTS` 期间调用需要再次 `AUX_MOUNTS` 的 VFS 操作；`remount_aux_readonly` / `unmount_aux_at` 亦同。

### 3.3 `ACTIVE_FS_IMPL` 持锁区间

```rust
let imp = ACTIVE_FS_IMPL.lock().ok_or(...)?;  // Guard 经 ok_or/? 转为 &dyn FsImpl 后 Guard 已 drop
let root = imp.mount_rw(device)?;
```

`mount_ro` / `mount_rw` 执行 ext4 探测与块 I/O 时**不**持有 `ACTIVE_FS_IMPL`，无同锁重入风险。

### 3.4 持锁闭环

| 路径 | 结论 |
|------|------|
| 所有 `AUX_MOUNTS` / `DEVICE_IDS` / `ROOT_*` 加锁 | 均为 RAII `MutexGuard`，正常/错误返回均释锁 |
| `mount_aux_common` 在 `assert_mount_point_directory` 失败时 | 未 push，无半开表项 |
| `clear_root_fs` | 三锁顺序固定，无遗漏释锁 |
| panic 路径 | `no_std` + 自旋锁无 poisoning；panic 后可能永久占锁（内核级问题，单列备注） |

---

## 4. 潜在问题（按严重程度）

### P0 — 死锁 / 卡死

| ID | 问题 | 分析 | 当前触发条件 |
|----|------|------|--------------|
| MR-01 | **未发现确定性 AB-BA 死锁** | 同线程不重入 `AUX_MOUNTS`；全局表与 Per-FS 锁分层释放 | — |
| MR-02 | **自旋锁 + 块 I/O 间接关联的长等待** | `assert_mount_point_directory` 在 mount 路径触发 ext4 `metadata`（可慢 I/O），虽不占 `AUX_MOUNTS`，但占 Per-FS 锁；其他线程 `resolve_route` 后等待同一 Per-FS 锁会自旋。单核下最终会轮到持有者，**非永久死锁**，但测试高并发 mount + 根卷 I/O 时可出现**长时间自旋（类卡死）** | 多线程同时 mount 与 heavy 根卷访问 |

### P1 — 数据竞争 / 语义偏差（高优先级）

| ID | 问题 | 位置 | 说明 |
|----|------|------|------|
| MR-03 | **`mount_aux_common` TOCTOU** | `mount_table.rs:165–181` | 重复挂载点检查与 `push` 非同一临界区；两线程并发 mount 同一路径可能均通过检查并 `push` 两条相同 `mount_point`，最长前缀路由行为未定义 |
| MR-04 | **`ROOT_RW_FS` / `ROOT_DEV_PATH` 非原子更新** | `lib.rs:91–92` | `mount_root_rw_from_block_path` 分两次加锁写入；读者可能在 `ROOT_RW_FS` 已设置、`ROOT_DEV_PATH` 仍为旧值（或 `None`）时调用 `root_identity` → 错误 minor / 设备路径 |
| MR-05 | **`clear_root_fs` 非原子清空** | `lib.rs:35–38` | 三次独立加锁；中间态下 `root_rw_fs()` 非空但 `ROOT_DEV_PATH` 已清，或反之 |
| MR-06 | **ext4 辅助挂载 double `bump_mount_generation`** | `mount_ext4_block_at` + `mount_aux_common` | 单次 mount bump 2 次；不 deadlock，但页缓存代次跳跃，与其他路径代次语义不一致 |

### P2 — 性能 / 覆盖范围

| ID | 问题 | 说明 |
|----|------|------|
| MR-07 | **`root_identity` 热路径双锁** | 每次根路径 VFS 操作：`ROOT_DEV_PATH` + `DEVICE_IDS`；高 QPS 根路径访问放大自旋 |
| MR-08 | **`resolve_route` 重复调用** | 写路径常 `assert_path_writable` + `fs_and_rel_rw` 各调一次，全局锁获取 ×2 |
| MR-09 | **无 mount/umount 全局串行化** | 并发 mount 不同点、umount 与 mount 竞态仅靠短临界区；除 MR-03 外无嵌套挂载冲突检测 |
| MR-10 | **`DEVICE_IDS` 只增不减** | 卸载不回收 minor；长期 LTP 可能膨胀（非锁死，但持锁遍历变长） |

### P3 — 备注

| ID | 问题 |
|----|------|
| MR-11 | bring-up 仅填 `ROOT_RW_FS`，`ROOT_FS` 常为空；`mount_aux_ro` 别名复用走 `root_fs()` 失败分支，属语义问题，非锁序问题 |
| MR-12 | `clear_root_fs` 不 bump `MOUNT_GENERATION`；与锁无关，但卸载根卷后页缓存可能 stale |

---

## 5. 当前实际支持范围

### 已正确加锁的路径

- 所有经 `resolve_route` 的 VFS 读/写/open/stat（`lib.rs`、`file_handle.rs`、`paged_handle.rs`）：先短持 `AUX_MOUNTS`，再 Per-FS。
- `sys_mount` / `sys_umount2` 经 `vfs::*` 进入 `mount_table`，无 syscall 层额外锁。
- 启动期 `set_active_fs_impl` → `mount_default_root_rw`：单线程 bring-up，无并发 mount。
- `remount_aux_readonly`、`unmount_aux_at`：持 `AUX_MOUNTS` 期间不回调 VFS。

### 未覆盖 / 不可靠路径

| 场景 | 状态 |
|------|------|
| 并发 `mount(2)` 同一 mount point | **未串行化**（MR-03） |
| 并发 mount + umount 相关子树 | 无 busy 检查；umount 不验证 open fd |
| 根卷 RW 挂载过程中的 `root_identity` 读者 | 可能读到 torn 状态（MR-04） |
| 运行时 `clear_root_fs` | API 存在，生产路径几乎未用；中间态风险未收敛 |
| 多核扩展 | 当前 baseline 单核；`AtomicU64` minor 分配无 CAS 冲突处理，多核需重审 |

---

## 6. 收敛建议

### MR-03：`mount_aux_common` TOCTOU

```rust
// 建议：检查 + assert_mount_point + push 置于同一 AUX_MOUNTS 临界区，
// 或 mount 全局静态 Mutex 串行化 mount/umount。
// assert_mount_point_directory 须在释锁后调用（需 VFS/Per-FS），
// 故推荐：检查后用 mount 序列号或「pending mount set」标记，
// push 前再次检查（double-check under lock）。
```

**Warn 收敛（若暂不改结构）**：

```rust
logging::warn!(
    "[lock:MountTable] concurrent mount not fully serialized; \
     mount_point={} caller=mount_aux_common",
    mp
);
// 返回 VfsError::Unsupported 或 Exists（若二次检查发现重复）
```

### MR-04 / MR-05：RootFsGlobals  torn read

- 将 `ROOT_RW_FS` + `ROOT_DEV_PATH`（及可选 `ROOT_FS`）合并为**单一** `spin::Mutex<RootFsState>`，或固定锁序并在单次临界区完成读写。
- `clear_root_fs` 与 mount 对称：同一结构体 + `bump_mount_generation`。

### MR-02：mount 路径自旋过长

- 在 `docs/exports/features/wateros-vfs.md` 标注：bring-up 阶段假定 mount 与 bulk I/O 不同时并发；LTP 若并行 mount + 压测，建议临时串行化 mount syscall。
- 可选：mount 路径用 `assert_mount_point_directory` 的轻量变体（仅 `exists` + 目录 bit，不走页缓存 overlay）。

### MR-06：double bump

- `mount_aux_*_from_block_path` 内移除 `bump_mount_generation`，仅保留 `mount_aux_common` / `unmount_aux_at` 一处 bump，避免代次双跳。

---

## 7. 与相邻锁域的边界

| 相邻结构 | 交互点 | 风险 |
|----------|--------|------|
| `SharedFs` Per-FS Mutex (#13) | `resolve_route` 之后 | 顺序正确；争用 Per-FS 非 mount 表 bug |
| `GlobalFilePageCache` (#12) | `mount_generation()`、`metadata` overlay | mount bump 后 cache 代次失效；double bump 放大失效 |
| `paged_handle` 锁序 | `FsPageIo::write_range` → `resolve_route` → fs lock | 与 mount 表无逆序 |
| `DEVFS` / block driver (#22) | `lookup_block_device` 在 mount 前 | 无 `AUX_MOUNTS` 交叉 |

---

## 8. 审计结论摘要

- **持锁闭环**：MountTable 与 RootFsGlobals 在正常与错误路径上均无显式漏释锁；`mount_aux_common` 刻意分段持锁以避免 `AUX_MOUNTS` 嵌套 VFS 死锁，设计合理。
- **主要风险**不在经典死锁，而在 **(1) mount 表 TOCTOU 并发插入**、**(2) RootFs 多 Mutex 非事务更新导致 torn read**、**(3) 自旋锁在 mount+I/O 叠加时的长自旋（类卡死体验）**。
- **单核 bring-up / 顺序 mount 测试**：当前实现可认为在控；**并发 mount(2) 或 mount 与根卷高压 I/O 并行**应视为未支持并需 warn + 收敛。

---

## 9. 高优先级修复列表（Top 3）

1. **MR-03** — `mount_aux_common` 重复挂载点检查与 `push` 非原子（TOCTOU）；并发 `mount(2)` 同路径可导致挂载表重复项与路由异常。
2. **MR-04** — `ROOT_RW_FS` 与 `ROOT_DEV_PATH` 分锁更新；根路径 `resolve_route` 的 `root_identity` 可能读到不一致设备身份（stat/mount_id/minor 错误）。
3. **MR-02** — mount 验证路径经完整 VFS `metadata` 获取 Per-FS 锁并可能触发块 I/O；与高负载根卷访问叠加时自旋等待时间不可控，表现为测试 intermittent 卡死，需串行化 mount 或轻量化检查。

---

## 10. P0 / P1 / Fixed 摘要

| 级别 | ID | 状态 | 简述 |
|------|-----|------|------|
| **P0** | MR-01 | 通过 | 未发现 `AUX_MOUNTS` ↔ `ROOT_*` / `DEVICE_IDS` 确定性 AB-BA 死锁；`mount_aux_common` 分段持锁避免 VFS 重入自锁 |
| **P0** | MR-02 | **开放** | mount 验证（`assert_mount_point_directory` → `metadata`）可长时间占 Per-FS 锁并触发块 I/O；与根卷高压 I/O 叠加时其他线程自旋等待不可控（单核非永久死锁，但类卡死） |
| **P1** | MR-03 | **开放** | `mount_aux_common` 重复点检查（`:166–170`）与 `push`（`:175–181`）非同一临界区；并发 mount 同路径可双插入 |
| **P1** | MR-04 | **开放** | `mount_root_rw_from_block_path` / `mount_root_from_block_path` 分锁写 `ROOT_RW_FS`/`ROOT_FS` 与 `ROOT_DEV_PATH`；`root_identity` 热路径可能 torn read |
| **P1** | MR-05 | **开放** | `clear_root_fs` 三次独立加锁清空；中间态下 `root_rw_fs()` 与 `current_root_device_path()` 不一致 |
| **P1** | MR-06 | **开放** | ext4 辅助挂载：`mount_aux_*_from_block_path` 与 `mount_aux_common` 各 `bump_mount_generation` 一次，代次双跳 |
| P2 | MR-07–MR-10 | 开放 | 热路径双锁、重复 `resolve_route`、`DEVICE_IDS` 只增不减等性能/覆盖项 |
| P3 | MR-11–MR-12 | 备注 | bring-up 仅 RW 根、`clear_root_fs` 不 bump 代次（语义，非锁序） |
| **Fixed** | — | **无** | 本轮未实施 warn 收敛或结构合并；源码与初审计一致 |

**持锁闭环结论**：全部 `AUX_MOUNTS` / `DEVICE_IDS` / `ROOT_*` / `ACTIVE_FS_IMPL` 路径均为 RAII 释锁，错误返回无漏释锁；**主要待修项为并发语义（MR-03/04/05）与 mount+I/O 争用体验（MR-02）**，非经典嵌套死锁。

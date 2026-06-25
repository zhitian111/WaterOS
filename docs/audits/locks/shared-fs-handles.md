# SharedFs / SharedRwFs 锁机制审计

> 审计项：lock-inventory #13  
> Baseline：单核多线程；`spin::Mutex` 自旋锁  
> 审计范围：`fs-api`、`vfs-impl/impl-fs-bridge`（含 `paged_handle`、`mount_table`）、`fs-impl/impl-ext4*`、`fs-rootfs/impl-kernel` 及直接持句柄的调用方  
> 生成时间：2026-06-25 · **复核更新**：2026-06-25

---

## 0. P0 / P1 / 已修复摘要

| 级别 | ID | 问题 | 状态 |
|------|-----|------|------|
| **P0** | FS-01 | aux RO 同块设备与 RW 根卷双 ext4 实例（数据损坏） | **已收敛** — `mount_aux_ro_from_block_path` 检测 `root_rw_fs()` 后 `warn` + `FsError::Unsupported` |
| **P0** | TRUNC-01 | `PagedFileHandle::truncate` 硬编码 `root_rw()`，aux 路径误操作 | **已修复** — 按 `resolve_route` 分 Root / AuxRw / AuxRo |
| **P1** | SFH-01 | 页缓存 flush 期间整卷 `SharedRwFs` 竞争（表现为 FS 卡死） | **未修复** — `entry.write()` 跨 batch + 多次 FS lock；Root `write_range` 已逐 chunk 释锁 |
| **P1** | SFH-02 | `EXT4_SMALL_READ_CACHE` 全局单例，多 FS 实例下脏读 | **未修复**（P0 收敛后 bring-up 主路径不可达；RO 根 + aux RW 同设备仍可达） |
| **P2** | SFH-03 | RO 根 + aux RW 同块设备可再 `mount_rw` 第二实例 | **未收敛** — `mount_aux_rw_from_block_path` 仅复用 `root_rw_fs()`，不拒绝 RO 根场景 |
| **P3** | SFH-04 | `MountedRwSession` 逐步独立 `lock()`，非原子事务 | 文档/设计偏差，非死锁 |
| **P3** | SFH-05 | `mount_rw_session` 仅绑定根卷 | 文档/设计偏差 |

**持锁闭环**：所有 `SharedFs` / `SharedRwFs` 路径均为 RAII `MutexGuard`，**无漏释锁**；静态槽位 Mutex 与实例 Mutex **不嵌套**（先克隆 Arc 再 `shared.lock()`）。

---

## 1. 数据结构概览

| 名称 | 定义位置 | 类型 | 保护对象 |
|------|----------|------|----------|
| `SharedFs` | `wateros-fs/fs-api/api-v0/src/lib.rs:494` | `Arc<spin::Mutex<LocalFs>>` | `Box<dyn ReadOnlyFs>`（ext4 RO 等） |
| `SharedRwFs` | 同文件 `:415` | `Arc<spin::Mutex<LocalRwFs>>` | `Box<dyn ReadWriteFs>`（ext4 RW、tmpfs 等） |
| `ROOT_RW_FS` | `fs-rootfs/rootfs-impl/impl-kernel/src/lib.rs:21` | `static Mutex<Option<SharedRwFs>>` | 全局根卷 RW 句柄槽位 |
| `ROOT_FS` | 同文件 `:20` | `static Mutex<Option<SharedFs>>` | 全局根卷 RO 句柄槽位（bring-up 主路径未使用） |
| `AUX_MOUNTS` | `vfs-impl/impl-fs-bridge/src/mount_table.rs:31` | `static Mutex<Vec<MountEntry>>` | 辅助挂载表；条目内嵌 `SharedFs` / `SharedRwFs` |

`LocalFs` / `LocalRwFs` 为 `Deref`/`DerefMut` 包装，**无独立锁**；所有 FS 实例级互斥由外层 `Arc<Mutex<...>>` 承担。

---

## 2. 句柄生命周期与挂载路径

```
块设备 ──FsImpl::mount_ro/rw──► Arc<Mutex<Local*Fs>>
                                    │
          mount_default_root_rw ────┼──► ROOT_RW_FS（全局）
          mount_aux_* ──────────────┼──► AUX_MOUNTS 条目（克隆 Arc）
          mount_tmpfs/cgroup ───────┘    本地 new Arc 后入表
```

**ext4 创建句柄**（`fs-impl/impl-ext4/src/lib.rs:69-82`，`impl-ext4-rs` 同模式 `:711-722`）：

- `mount_ro` → `Arc::new(Mutex::new(LocalFs::new(Box::new(Ext4Fs))))`
- `mount_rw` → `Arc::new(Mutex::new(LocalRwFs::new(Box::new(Ext4FsRw))))`

**同设备 alias 复用与收敛**（`rootfs-impl/impl-kernel/src/lib.rs:125-181`）：

| 路径 | 行为 |
|------|------|
| `mount_aux_rw_from_block_path` | `Arc::ptr_eq` 同设备 → **复用 `root_rw_fs()` Arc**（共享同一 `Mutex`） |
| `mount_aux_ro_from_block_path` | 同设备且 `root_fs()` 存在 → 复用 RO 句柄 |
| `mount_aux_ro_from_block_path` | 同设备且 **`root_rw_fs()` 存在、`root_fs()` 为空**（bring-up 主路径）→ **`warn` + `Unsupported`，禁止第二 RO 实例** |
| `mount_ext4_block_at(..., readonly=true)` | 经上路径；同设备 bind RO 在 RW 根下**挂载失败**（安全） |
| `remount_aux_readonly` | 仅设 `MountEntry.readonly` 标志；底层仍为同一 `SharedRwFs` Arc（bind alias 正确） |

当前 bring-up 仅 `mount_default_root_rw`，`ROOT_FS` 恒为 `None`；同设备 aux **RO** bind 已被拒绝；同设备 aux **RW** bind 共享根 Arc。

---

## 3. 锁顺序约定（与页缓存）

`paged_handle.rs` 与 `impl-page-cache/src/lib.rs` 文档化顺序（编号越小越先获取）：

1. `page_cache.files`（`Mutex`，极短）
2. per-file `FileEntryInner`（`RwLock` read/write）
3. `page_cache.state`（`Mutex`，极短；**持锁期间不得 I/O**）
4. **`SharedRwFs` / `SharedFs`**（仅在 `PageCacheIo::read_range` / `write_range` 或 VFS bridge 单次 FS 调用内短持）

**禁止**：持 `SharedRwFs` 后再等待页缓存 entry 锁（历史 dead-lock 根因，已在 flush 路径修复）。

实际下探链（flush miss 为例）：

```
PagedFileHandle::sync_dirty
  └─ page_cache.flush [entry.write() 跨 batch]
       └─ flush_dirty_run [drop state]
            └─ FsPageIo::write_range
                 └─ SharedRwFs::lock → ext4 → SharedBlockDevice::lock
```

VFS 元数据路径：`SharedRwFs::lock` 在 `match` 表达式内释放 **之后** 才调用 `overlay_cached_size`（不违反顺序）。

---

## 4. `lock()` 调用点清单

### 4.1 全局槽位（`ROOT_*` 静态 Mutex）

| 函数 | 文件 | 持锁内容 |
|------|------|----------|
| `root_rw_fs()` | `rootfs-impl/impl-kernel/lib.rs:105` | 克隆 `Option<SharedRwFs>` |
| `set_root_fs` / `clear_root_fs` | `:29-38` | 读写 `ROOT_FS` / `ROOT_RW_FS` |
| `mount_root_rw_from_block_path` | `:91` | 写入 `ROOT_RW_FS` |
| `mount_aux_*` | `:125-181` | 读 `ACTIVE_FS_IMPL`；alias 时读 `root_fs`/`root_rw_fs` |

静态 Mutex 与实例 Mutex **不嵌套**：`root_rw()` 先克隆 `Arc` 释放入口锁，再 `shared.lock()`。

### 4.2 VFS bridge（`impl-fs-bridge/src/lib.rs`）

| 操作 | 路由 | 锁目标 |
|------|------|--------|
| `exists` / `metadata` / `read` / `read_dir` / `read_symlink` | `FsRoute::Root` | `root_rw()?.lock()` |
| 同上 | `AuxRw` / `AuxRo` | `fs.lock()`（挂载表内 Arc） |
| `read_range` | 三路 + PseudoProc | 同上 |
| `read_dir_on_root` | 仅根 | `root_rw()?.lock()` |
| `MountedRwSession::*` | 写 syscall 委托 | 每次调用 `self.inner.lock()`（独立短持） |
| `fs_and_rel_rw` | `AuxRw { readonly: true }` | 返回 `ReadOnlyFs`，**不** lock |

读路径：**每次 VFS 调用一次 lock → 单次 FS trait 调用 → 自动 unlock**；无跨调用持锁。

### 4.3 页缓存 I/O 委托（`paged_handle.rs`）

| 函数 | 行 | 行为 |
|------|-----|------|
| `FsPageIo::read_range` | `:33-34` | 委托 `FsBridge.read_range`（内部按需 lock） |
| `FsPageIo::write_range` Root | `:43-54` | **`while` 内每 chunk 独立 `rw.lock()`**，chunk 结束 guard drop |
| `FsPageIo::write_range` AuxRw | `:57-58` | `fs.lock().write_range` **单次**持锁写整 buffer |
| `PagedFileHandle::truncate` | `:407-425` | **`resolve_route`** → Root / AuxRw 分别 lock |

### 4.4 挂载表（`mount_table.rs`）

| 操作 | 与 SharedFs/RwFs 关系 |
|------|----------------------|
| `longest_aux_mount` | 持 `AUX_MOUNTS` 锁，**克隆** Arc 后释放 |
| `mount_tmpfs_at` / `mount_cgroup_at` | `Arc::new(Mutex::new(LocalRwFs::new(...)))` 新建句柄 |
| `mount_aux_at_rw/ro` | 接收外部传入的 `SharedRwFs` / `SharedFs` |
| `assert_mount_point_directory` | 间接：`bridge.metadata` → 可能 lock 根/aux FS（**不在** `AUX_MOUNTS` 持锁期间） |

### 4.5 ext4 实现内部（持 **`SharedRwFs` 锁期间**嵌套）

| 锁 | 文件 | 说明 |
|----|------|------|
| `SharedBlockDevice::lock` | `rw.rs` `block_write_bytes`、`read_with_small_cache`；`ro.rs` `BlockDeviceReader` | 块 I/O |
| `EXT4_SMALL_READ_CACHE` | `rw.rs:24-112` | **全局单例** 4KiB 读缓存；写路径先 invalidate 再 device lock |

**嵌套顺序（RW 小读）**：`EXT4_SMALL_READ_CACHE` ↔ `SharedBlockDevice` 可能交叉（见 lock-issues **BLK-01**）；在单 `SharedRwFs` 实例下由外层 FS Mutex 串行化。

### 4.6 其他调用方

| 调用方 | 用法 |
|--------|------|
| `mm-impl/*/kernel_elf.rs` | `root_fs().lock().read(path)`（RO 槽；`vfs-root-read` feature 走 VFS） |
| `impl-ext4/selftest.rs` | 测试内 `fs.lock()` / `rw.lock()` |

---

## 5. 潜在问题（按严重程度）

### 5.1 [P0 / 已收敛] aux RO 同设备双 ext4 实例

**位置**：`mount_aux_ro_from_block_path`（`rootfs-impl/impl-kernel/src/lib.rs:138-144`）

**原问题**：alias 仅查 `root_fs()`；bring-up 仅 RW 根时同设备 aux RO 会再 `mount_ro` 独立 `Ext4Fs`，与 `Ext4FsRw` 并发写同一 `SharedBlockDevice`。

**当前行为**：

```rust
if root_rw_fs().is_some() {
    logging::warn!(
        "[fs::rootfs] mount aux RO rejected: same block device as active RW root ({})",
        path
    );
    return Err(fs_api_v0::FsError::Unsupported);
}
```

**结论**：bring-up（RW 根 + bind RO 同设备）**已收敛**；`mount_ext4_block_at(..., readonly=true)` 安全失败。

---

### 5.2 [P0 / 已修复] `PagedFileHandle::truncate` 路由

**位置**：`paged_handle.rs:407-425`

**原问题**：硬编码 `root_rw()?.lock().truncate`，aux 卷 `ftruncate` 误操作根卷。

**当前行为**：`resolve_route` 分 `Root` / `AuxRw` / `AuxRo|PseudoProc`（只读返回 `ReadOnlyFs`）。

**结论**：**已修复**。

---

### 5.3 [P1 / 未修复] 页缓存 flush 期间整卷 FS 竞争

**位置**：

- `impl-page-cache` `flush`：持 `entry.write()` 收集 dirty 列表，跨 `flush_dirty_run` batch（`FLUSH_RUN_MAX_PAGES=64`）
- `FsPageIo::write_range` Root：每 chunk 释 FS 锁，但 flush 可连续多 batch
- `FsPageIo::write_range` AuxRw：**单次** `fs.lock()` 写整 batch（Aux 路径持锁可能更长）

**现象**：flush 大文件时其他任务（`exists`、`mkdir`、另一文件 read miss）需 spin 等待 `SharedRwFs`；表现为**整卷 FS 卡死**（非死锁，长自旋）。

**收敛建议**：

1. Aux `write_range` 与 Root 对齐：按 chunk 分段 `lock()` / drop。
2. `flush_dirty_run`：batch 间 `yield` 或减小 `FLUSH_RUN_MAX_PAGES`。
3. 超大 flush **`warn`**：`SharedRwFs`、`write_range`、`path`、字节数。

---

### 5.4 [P1 / 未修复] `EXT4_SMALL_READ_CACHE` 全局单例

**位置**：`impl-ext4/src/rw.rs:24`

**现象**：缓存 keyed by `(dev_id, block)`，**非 per-Ext4FsRw**。两个 `SharedRwFs` 实例（§5.5 边缘路径）交替读时可能返回错误块。

**Baseline**：P0 收敛后 RW 根 + aux RO 不可双实例；主路径单实例。**RO 根 + aux RW 同设备**仍可双实例。

**收敛建议**：缓存移入 `Ext4FsRw` 实例字段；或 §5.5 一并拒绝第二 RW 挂载。

---

### 5.5 [P2 / 未收敛] RO 根 + aux RW 同块设备

**位置**：`mount_aux_rw_from_block_path`（`:158-181`）

**现象**：`ptr_eq` 同设备时仅尝试复用 `root_rw_fs()`；若根仅为 `ROOT_FS`（RO），会 **`mount_rw` 第二 `Ext4FsRw` 实例**，与 RO `Ext4Fs` 共享块设备、**独立 Mutex**。

**触发**：非 bring-up 路径（`mount_default_root` RO + bind RW 同设备）。当前 bring-up **不可达**。

**收敛建议**：同设备且无 `root_rw_fs` 时 `warn` + `Unsupported`，或强制升级为单 RW 句柄。

---

### 5.6 [P3] `MountedRwSession` 无会话级持锁

**位置**：`lib.rs:580-694`

**现象**：每个 trait 方法独立 `lock()`；`overwrite_file_at`、`rename_path` 等多步**非原子**。

**结论**：非死锁；单核 bring-up 低概率。文档标明「单次 syscall 粒度」。

---

### 5.7 [P3] `mount_rw_session` 仅绑定根卷

**位置**：`lib.rs:703-705`

**现象**：始终 `MountedRwSession::new(root_rw()?)`；aux 写须走 `mkdir_path` / 路由 API。

---

### 5.8 [低 / 持锁闭环] RAII

**结论**：所有 `MutexGuard` RAII 释锁；错误路径靠 drop。**无漏释锁**。  
**关注**：guard 存活期间 ext4 为自旋 + 块 I/O 轮询，当前无睡眠；若未来引入阻塞 I/O 须重审。

---

## 6. 持锁区间分析摘要

| 路径 | 持锁区间 | 睡眠/调度 | 嵌套锁 |
|------|----------|-----------|--------|
| VFS 单次 read/metadata | 一次 trait 调用 | 否 | → BlockDevice |
| `MountedRwSession` 写 | 单次 syscall | 否 | 同上 |
| 页缓存 read miss | entry.read + 多次短 FS lock | 否 | 顺序合规 |
| 页缓存 write/flush | entry.write + 多次 FS lock（可能很长） | 否 | 顺序合规；**整卷阻塞** |
| ext4 `read_with_small_cache` | FS lock 内：Cache ↔ Device | 否 | 见 BLK-01 |
| ELF 加载 | `root_fs().lock()` 整文件 read | 否 | BlockDevice |

**与页缓存死锁**：当前**未发现**持 `SharedRwFs` 后再抢 entry 锁的逆序路径。

---

## 7. 当前实际支持范围

| 场景 | 锁行为 | 可靠性 |
|------|--------|--------|
| 单 RW ext4 根卷 + VFS read/write | 全局单 `SharedRwFs`；页缓存短持 FS 锁 | **主路径，已覆盖** |
| tmpfs / cgroup aux 挂载 | 独立 `Arc<Mutex>` per 挂载 | **读写经路由正确**；truncate 已修复 |
| bind mount 同设备 RW alias | 共享根 `SharedRwFs` Arc | **有意共享** |
| bind mount 同设备 RO（RW 根 bring-up） | 拒绝挂载 | **已收敛** |
| bind mount 同设备 RW（RO 根） | 可第二 RW 实例 | **未收敛，边缘** |
| `mount_rw_session` API | 仅根卷 | **部分覆盖** |
| 多线程并发同卷 | `spin::Mutex` 互斥 | **可接受**；长 flush 易卡死（P1） |
| `SharedFs` RO 根（`ROOT_FS`） | 与 RW 双槽并存 | **bring-up 未启用** |

---

## 8. 收敛优先级汇总

| 优先级 | ID | 问题 | 建议动作 | 状态 |
|--------|-----|------|----------|------|
| P0 | FS-01 | aux RO 同设备双实例 | warn + 拒绝 | **已收敛** |
| P0 | TRUNC-01 | truncate 错路由 | `resolve_route` | **已修复** |
| P1 | SFH-01 | flush 长竞争 FS 锁 | 分段释锁 / 缩小 batch | 待实现 |
| P1 | SFH-02 | 全局小读缓存 | 移入 `Ext4FsRw` | 待实现 |
| P2 | SFH-03 | RO 根 + aux RW 双实例 | 同 FS-01 策略 | 待实现 |
| P3 | SFH-04/05 | 会话非原子 / session 仅根 | 文档 | 已知限制 |

---

## 9. 建议 warn 模板（待主 agent 统一）

```rust
log::warn!(
    "[lock-audit] SharedRwFs op={} path={} ctx={} — 路径未完整支持，安全失败",
    "mount_aux_ro", block_path, "same-device-as-rw-root"
);
```

字段要求（对齐 `audit_lock_mechanisms.md`）：数据结构名、锁操作类型、函数/文件、上下文参数。

---

## 10. 相关文件索引

| 组件 | 路径 |
|------|------|
| API 定义 | `os/components/wateros-fs/fs-api/api-v0/src/lib.rs` |
| ext4 挂载 | `os/components/wateros-fs/fs-impl/impl-ext4/src/lib.rs` |
| ext4 RW + 块缓存 | `os/components/wateros-fs/fs-impl/impl-ext4/src/rw.rs` |
| 根卷槽位 / 双实例收敛 | `os/components/wateros-fs/fs-rootfs/rootfs-impl/impl-kernel/src/lib.rs` |
| VFS bridge | `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/lib.rs` |
| 页缓存句柄 | `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/paged_handle.rs` |
| 挂载表 | `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/mount_table.rs` |
| 页缓存锁序 | `os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs` |

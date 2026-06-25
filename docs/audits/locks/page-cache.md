# GlobalFilePageCache 锁机制审计

> 审计日期：2026-06-25（复审：2026-06-25，对照 commit `b6e6d01`）  
> Baseline：单核多线程；`spin::Mutex` / `spin::RwLock` 为自旋锁，**不可重入**  
> 清单编号：lock-inventory #12  
> 主要源文件：`os/components/wateros-vfs/vfs-impl/impl-page-cache/src/lib.rs`  
> 主要调用方：`os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/paged_handle.rs`、`impl-fs-bridge/src/lib.rs`

---

## 1. 数据结构概览

### 1.1 GlobalFilePageCache

| 字段 | 锁类型 | 保护内容 |
|------|--------|----------|
| `state` | `spin::Mutex<GlobalCacheState>` | 页帧池、LRU、`index` 映射 |
| `files` | `spin::Mutex<BTreeMap<FileCacheKey, Arc<RwLock<FileEntryInner>>>>` | 路径 → 文件元数据条目 |
| `open_refs` | `spin::Mutex<BTreeMap<FileCacheKey, usize>>` | 仍被 `PagedFileHandle` 持有的路径引用计数 |

### 1.2 每文件 FileEntryInner

| 字段 | 锁类型 | 保护内容 |
|------|--------|----------|
| `Arc<RwLock<FileEntryInner>>` | `spin::RwLock` | `logical_size`、`dirty_pages` |

### 1.3 全局静态量 GLOBAL_CACHE

```rust
static GLOBAL_CACHE: Mutex<Option<Arc<GlobalFilePageCache>>> = Mutex::new(None);
```

- `global_cache(mount_gen)`：加锁 → 按需重建 → 克隆 `Arc` → 释锁  
- `reset_global_cache(mount_gen)`：加锁 → 整体替换为新实例 → 释锁  

---

## 2. 文档化锁顺序

源码注释（`lib.rs` L5–15、`paged_handle.rs` L3–10）约定：

| 序号 | 锁 | 约束 |
|------|-----|------|
| 1 | `files` Mutex | 极短持有 |
| 2 | per-file `FileEntryInner` RwLock | read / write |
| 3 | `state` Mutex | 极短持有；**持锁期间不得调用下层块设备 I/O** |
| 4 | `SharedRwFs`（ext4） | 仅在 `PageCacheIo::read_range` / `write_range` 内短持 |

**未纳入顺序的锁**：`open_refs` Mutex、`GLOBAL_CACHE` Mutex。

**禁止**：持 ext4 锁后再等待页缓存 entry 锁（历史死锁根因，已在 `paged_handle` 注释中说明）。

**驱逐写回约束**（`b6e6d01` 后）：`install_page` / `install_zero_page` 在 LRU 驱逐脏页时可在锁外调用 `logical_size_for_key` 与 `writeback_evicted_page` → `note_page_written_back`；**调用方不得在持 entry 锁期间进入 `install_page`**。

---

## 3. 锁操作调用点清单

### 3.1 GLOBAL_CACHE

| 函数 | 操作 | 持锁区间 |
|------|------|----------|
| `global_cache` | lock → 可能 rebuild → clone Arc → unlock | 每次 VFS 读写/metadata 入口均调用 |
| `reset_global_cache` | lock → 赋值新实例 → unlock | 挂载切换 / `reset_file_page_cache` |

### 3.2 open_refs

| 函数 | 操作 |
|------|------|
| `acquire_open_ref` | lock → 递增 → unlock |
| `release_open_ref` | lock → 递减 → unlock；count==0 时**锁外**调 `purge_closed_file` |
| `purge_closed_file` | lock → remove key → unlock（**不检查 count**） |

### 3.3 files

| 函数 | 操作 |
|------|------|
| `get_file_entry` | lock → 查/插 → **嵌套** entry.read/write（仅 size 升级） → unlock |
| `logical_size_for_key` | lock → entry.read → unlock |
| `note_page_written_back` | lock → entry.write → unlock |
| `flush` | lock → clone Arc → unlock |
| `flush_all` | lock → 收集路径 → unlock |
| `logical_size` / `dirty_page_count` | lock → entry.read → unlock |
| `purge_closed_file` | lock → remove → unlock |

### 3.4 state

| 函数 | 操作 |
|------|------|
| `install_page` / `install_zero_page` | 多次 lock/unlock；驱逐脏页前 `drop(state)` 再 I/O |
| `read` / `write` 循环 | 每页 brief lock 读/写帧 |
| `flush_dirty_run` | 每页多次 lock；I/O 在锁外 |
| `purge_closed_file` / `truncate` | lock → 清理 index/LRU/free → unlock |

### 3.5 per-file RwLock

| 函数 | 模式 | 持锁区间 |
|------|------|----------|
| `read` | **无** entry 锁 | 仅 `get_file_entry` 时可能短暂嵌套；`install_page` 在锁外 |
| `write` | entry.read（取 logical_size）→ **释锁** → install → entry.write（dirty/size）→ 释锁 | 每页粒度，**install 期间不持 entry 锁** |
| `flush` | entry.write（收集 dirty 列表）→ **释锁** → flush_dirty_run → entry.write（清除已刷页） | flush_dirty_run / install 在锁外 |
| `truncate` | entry.write() | 更新 logical_size / dirty_pages |
| `get_file_entry` | 嵌套 read/write | 在 **files 仍持锁** 时，仅 size 升级 |

---

## 4. 调用链与持锁区间分析

### 4.1 读路径（`b6e6d01` 后）

```
PagedFileHandle::read
  → global_cache()                    [GLOBAL_CACHE 短持]
  → GlobalFilePageCache::read
      → get_file_entry                [files + 可能嵌套 entry，随即释 files]
      → install_page (每页，无 entry 锁)
          → state lock/unlock
          → PageCacheIo::read_range     [ext4 锁，state 已释]
          → writeback_evicted_page?     [驱逐时，无外层 entry 锁]
              → logical_size_for_key    [files + entry.read]  ✓
              → note_page_written_back  [files + entry.write] ✓
      → state lock → 拷贝 → unlock
```

### 4.2 写路径（`b6e6d01` 后）

```
PagedFileHandle::write
  → cache.write
      → get_file_entry
      → entry.read()（取 logical_size）→ 释锁
      → install_page / install_zero_page（无 entry 锁）
          → writeback_evicted_page → note_page_written_back  ✓
      → state lock → 写脏页 → unlock
      → entry.write()（dirty_pages / logical_size）→ 释锁
```

### 4.3 flush / close 路径（`b6e6d01` 后）

```
PagedFileHandle::close
  → sync_dirty → cache.flush
      → files lock（clone entry）→ unlock
      → entry.write()（收集 dirty + logical_size）→ 释锁
      → flush_dirty_run（无 entry 锁）
          → install_page（缺页时）→ writeback_evicted_page  ✓
          → io.write_range（ext4）
      → entry.write()（清除已刷 dirty_pages）
  → release_open_ref
      → open_refs lock
      → purge_closed_file（末引用）   [open_refs + files + state]
```

`PagedFileHandle::flush` / `truncate`（len>0）同样经 `sync_dirty` 进入上述 flush 链。

### 4.4 全局回收路径

```
reset_file_page_cache
  → global_cache → flush_all（逐路径 flush）
  → reset_global_cache（丢弃旧 Arc）

unlink_path / overwrite_file_at
  → purge_closed_file（无 flush）

mount_table 辅助挂载/卸载/remount
  → bump_mount_generation
  → 下次 global_cache() 可能 silent rebuild（无 flush）
```

---

## 5. 潜在问题（按严重程度）

### ~~P0~~ — 持 entry 锁期间重入 entry 锁（自旋自死锁） — **已修复（`b6e6d01`）**

**原位置**：`note_page_written_back`（L224–231）、`logical_size_for_key`（L255–262），由 `writeback_evicted_page` → `install_page` 在 read/write/flush 持锁期间触发。

**原机制**：read/write/flush 全程持 entry RwLock 时，驱逐写回调再次申请 entry 锁 → spin 自死锁。

**修复**（`impl-page-cache/src/lib.rs`，commit `b6e6d01`）：

1. **`read`**：移除全程 `entry.read()`；`install_page` 在锁外执行。  
2. **`write`**：改为每页 brief `entry.read()` 取 `logical_size` 后立即释锁；`install_page` 完成后才 brief `entry.write()` 更新 dirty。  
3. **`flush`**：收集 dirty 列表后释锁；`flush_dirty_run`（含缺页 `install_page` 与驱逐）在锁外；每批刷完后 brief `entry.write()` 清除 dirty 标记。  
4. **`writeback_evicted_page`**：`logical_size` 改由 `install_page` 在锁外经 `logical_size_for_key` 传入，避免在已持 entry 锁的上下文中嵌套 lookup。

**残余注意**：`note_page_written_back` 仍走 `files` → `entry.write()`，但仅在**无外层 entry 锁**的 `install_page` 驱逐路径调用，不再自死锁。

---

### ~~P0~~ — flush 持 entry.write 期间 flush_dirty_run 重入 install_page 驱逐链 — **已修复（`b6e6d01`）**

与上一项同一提交：`flush` 不再在 `flush_dirty_run` 全程持 `entry.write()`，close/fsync 路径在缓存饱和 + 驱逐场景下可完成。

---

### P1 — global_cache(mount_gen) 代次不匹配时 silent rebuild 丢弃脏页

**位置**：`global_cache`（L825–836）；`mount_table` 中 `bump_mount_generation`（挂载/卸载/remount 辅助卷）。

**机制**：当前 GLOBAL_CACHE 中实例 mount_gen 与参数不符时，直接 `GlobalFilePageCache::new(mount_gen)` 替换，**不 flush** 旧实例。仅 `reset_file_page_cache` 在 rebuild 前显式 `flush_all`。

**连带问题**：`PagedFileHandle` 在 open 时快照 `mount_gen`；挂载代次 bump 后，旧句柄调用 `global_cache(old_gen)` 会把 GLOBAL_CACHE **回退/重建为旧代次**，与当前活跃代次不一致，导致缓存视图错乱。

**收敛建议**：

1. `global_cache` rebuild 前若旧实例 `dirty_page_count` 合计 > 0，warn + 尝试 `flush_all` 或拒绝 rebuild。  
2. 代次 bump 时统一调用 `reset_file_page_cache`（或等价 flush+reset）。  
3. 旧 mount_gen 句柄在代次变更后应失效（返回错误或强制 detached 模式）。

---

### P1 — purge_closed_file 无 flush、无视 open_refs，unlink 路径可丢脏数据

**位置**：`purge_closed_file`（L713–749）；`unlink_path`（L441–444）；`overwrite_file_at`（L553–554）。

**机制**：

- `purge_closed_file` 直接 `open_refs.remove`（不检查计数），移除 `files` 条目并回收 `state` 中页帧，**不 writeback**。  
- 注释要求「close/unlink 之后调用」，但 `unlink_path` 未先 flush、未验证无 open fd。  
- 若路径仍有未 close 的 fd 或脏页未 flush，缓存脏数据被丢弃。

**收敛建议**：

1. `purge_closed_file` 前检查 `open_refs.get(key).copied().unwrap_or(0) == 0`，否则 warn + 跳过或返回错误。  
2. `unlink_path` 在 purge 前对仍缓存的脏页调用 `flush`（或依赖 VFS 层「文件仍打开则 unlink 失败」语义并文档化）。  
3. purge 时若 `state` 帧 `dirty==true`，warn + 同步写回或拒绝 purge。

---

### P2 — get_file_entry 在持 files 锁时嵌套 entry RwLock

**位置**：`get_file_entry`（L180–194）。

**机制**：违反「files 极短持锁」约定；在 `initial_size > logical_size` 时于 files 保护下调用 `entry.write()`。与 `note_page_written_back`（files → entry.write）锁顺序相同，一般不形成 AB-BA，但延长 files 临界区，放大与 `flush_all` / `logical_size` 等路径的互斥等待。

**收敛建议**：files 锁内仅 clone/insert Arc；size 升级移到 files 释锁后的 entry.write 中。

---

### P2 — open_refs 未纳入全局锁顺序文档

**位置**：全局锁顺序注释 vs `purge_closed_file`（open_refs → files → state）。

**机制**：`acquire_open_ref` / `release_open_ref` 与 `files` 无固定相对顺序；当前实现因 `release_open_ref` 在 open_refs 释锁后才 purge，暂无明确死锁环，但后续维护易引入 `files` → `open_refs` 逆序。

**收敛建议**：将 `open_refs` 编为序号 0（与 files 互斥使用，禁止同时持有）或序号 1.5 并文档化「不得与 files 交叉嵌套」。

---

### P2 — GLOBAL_CACHE 每操作加锁

**位置**：所有 `global_cache()` 调用（read/write/open/close/metadata 等）。

**机制**：每次 I/O 操作一次静态 Mutex + Arc clone；单核多线程下竞争时自旋等待，非死锁但增加延迟；与 `PerTaskFdRegistry` 路径叠加时放大卡死感知。

**收敛建议**：启动时或首次 mount 后缓存 `Arc<GlobalFilePageCache>` 于 `PagedFileHandle`（已有 `mount_gen`，可存 `Arc` 避免重复 lock GLOBAL_CACHE）。

---

### P2 — flush 与 write 并发时 dirty 列表快照语义

**位置**：`flush`（L648–690）在释锁后执行 `flush_dirty_run`。

**机制**：收集 dirty 列表后释 entry 锁，并发 `write` 可能新增脏页；当次 flush 不保证刷尽所有脏页（需再次 flush）。非死锁，属语义/一致性偏差。

**收敛建议**：文档化「单次 flush 为 best-effort 快照」；或 flush 末尾校验 `dirty_page_count` 非零则 warn/重试。

---

### P3 — rename 未同步页缓存

**位置**：`rename_path`（L567–577）无 `purge` / key 迁移。

**机制**：重命名后旧路径缓存条目残留，新路径冷启动；若旧路径 fd 仍打开，行为依赖 stale path 字符串，属语义偏差而非直接死锁。

---

## 6. 当前实际支持范围

| 路径 | 锁覆盖 | 可靠性 |
|------|--------|--------|
| 单 fd 顺序 read/write（缓存未满、无驱逐） | files / entry / state / ext4 分层基本正确 | **较可靠** |
| 多 fd 同路径 dup（open_ref 计数） | acquire/release + close 末引用 purge | **较可靠**（需 close 配对） |
| close / flush / fsync | flush 链在锁外执行 flush_dirty_run / install | **较可靠**（`b6e6d01` 后；仍缺饱和驱逐集成测） |
| 缓存饱和 + 读写并发 | 驱逐写回不再与 entry 锁重入 | **较可靠**（`b6e6d01` 后） |
| 辅助卷 mount/unmount 后 IO | mount_gen bump + global_cache rebuild | **不可靠（P1 丢脏数据/代次错乱）** |
| unlink / overwrite 后缓存 | purge 无 flush | **不可靠（P1）** |
| reset_file_page_cache | flush_all + reset | **可靠**（前提是调用点安全、无并发 fd） |
| truncate | sync_dirty + ext4 truncate + cache.truncate | **较可靠**（sync 阶段已不受 P0 影响） |
| Direct 模式（FILE_IO_MODE=Direct） | 设计目标路径 | Async 模式 open 直接 Unsupported |

---

## 7. 与 ext4 / SharedRwFs 的锁协作

**已做对**：

- `install_page` / `install_zero_page` 在 `read_range` / `write_range` 前 `drop(state)`。  
- `paged_handle` 注释明确禁止 ext4 锁 → entry 锁顺序。  
- `FsPageIo::write_range` 仅在调用期间短持 `root_rw().lock()`。  
- read/write/flush 不再在 entry 锁内调用 `install_page`（`b6e6d01`）。

**仍须注意**：

- flush / 驱逐写回期间虽无 entry 锁，但同文件并发 write 与 flush 可能交错 dirty 快照（P2）。  
- `truncate` 先 flush（entry write 短持）再 ext4 truncate，顺序正确。

**说明**：commit `b6e6d01` 中 `paged_handle.rs` 变更为 aux 卷 `truncate` 路由，**非** entry 锁修复；entry 锁修复均在 `lib.rs`。

---

## 8. 收敛修复优先级建议

| 优先级 | 项 | 状态 | 建议动作 |
|--------|-----|------|----------|
| 1 | P0 entry 锁重入 | **已修复** | — |
| 2 | P0 flush 持锁驱逐 | **已修复** | — |
| 3 | P1 mount_gen rebuild | 开放 | bump 代次时强制 flush；`global_cache` 禁止 silent 丢脏 |
| 4 | P1 purge 无 flush | 开放 | unlink/purge 前 flush 或校验 open_refs==0 |
| 5 | P2 get_file_entry 嵌套 | 开放 | 缩短 files 临界区 |
| 6 | P2/P3 | 开放 | 文档补 open_refs 顺序；rename 缓存失效；flush 快照语义 |

---

## 9. 相关测试

`impl-page-cache` 单元测试（L838–1006）覆盖：整页写、flush 合并、purge、release_open_ref，**未覆盖**：

- 容量饱和下 read/write/flush 与驱逐交错（P0 回归）  
- mount_gen 变更与 global_cache rebuild  
- unlink 与 open fd / 脏页交互  
- 多线程（单核抢占）同文件并发  

建议在 `FILE_PAGE_CACHE_CAPACITY` 设为 2–4 页时增加「写满 → 读另一文件 → 触发驱逐 → close」集成测例以锁定 P0 修复。

---

## 10. 审计结论摘要

| 级别 | 项 | 状态 |
|------|-----|------|
| **P0** | entry RwLock 重入自死锁（read/write/flush + LRU 驱逐） | **已修复**（`b6e6d01`） |
| **P0** | flush 持 entry.write 期间 flush_dirty_run → install_page 驱逐 | **已修复**（`b6e6d01`） |
| **P1** | `global_cache` silent rebuild 丢脏 / mount_gen 回退 | 开放 |
| **P1** | `purge_closed_file` 无 flush / unlink 丢脏 | 开放 |
| **P2** | `get_file_entry` files 嵌套 entry、open_refs 顺序、GLOBAL_CACHE 热路径、flush 快照语义 | 开放 |
| **P3** | rename 未失效页缓存 | 开放 |

`GlobalFilePageCache` 三层 Mutex + per-file RwLock 的分层设计方向正确，I/O 与 `state` 分离符合文档约定。**历史最危险缺陷**（持 entry 锁期间驱逐写回重入 entry 锁导致 spin 自死锁）已在 `b6e6d01` 通过缩短 entry 持锁区间、flush 锁外执行 `flush_dirty_run` 消除。当前主要剩余风险为 mount 代次变更与 purge 路径的**脏页丢失**及 **open_refs** 语义不一致（P1）。

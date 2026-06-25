# 锁机制审计：fs-aux（DEVFS / DEV_NODES / ProcfsLookups / EXT4_SMALL_READ_CACHE）

> 审计日期：2026-06-25（初稿）；**复核源码：2026-06-25**  
> 清单编号：#18–#21（`docs/audits/lock-inventory.md`）  
> Baseline：单核多线程；`spin::Mutex` 为自旋锁，持锁期间不得睡眠/让出 CPU（否则同核其他线程永久自旋）。

---

## P0 / P1 / 已修复摘要

| 优先级 | ID | 结构 | 问题 | 状态 |
|--------|-----|------|------|------|
| ~~P0~~ | **PROC-01** | `ARGV_LOOKUP` / `EXE_LOOKUP` / `MOUNT_LOOKUP` | 持 lookup 自旋锁期间执行 cwd/VFS 回调 → 抢占下永久自旋；回调重入 proc 时同锁自死锁 | **已修复**（锁内复制 `Option<fn>`，锁外调用，见 §4.1） |
| **P0** | **DEV-01** | `DEVFS` | `refresh` 长临界区（嵌套 `BLOCK_DEVICES`/`CHARACTER_DEVICES` + 堆分配 + 日志）；`clear` 后重建前 lookup 空窗 | **未修复** |
| **P1** | **CACHE-01** | `EXT4_SMALL_READ_CACHE` | 全局单 Mutex + `SharedRwFs` + 块设备三层嵌套 → 多任务 ext4 小读争用自旋 | **未修复**（性能/卡死风险，非 AB-BA） |
| **P1** | **PROC-02** | Procfs 回调链 | cwd 回调仍用 `UniprocessorSafeCell`；抢占下 `RefCell already borrowed` panic | **未修复**（RC-2 关联） |
| **P1** | **DEV-02** | `DEV_NODES` | `refresh` 持锁堆分配；测试环境多线程 refresh 自旋 | **未修复**（非 kernel 主路径） |
| ~~P0~~ | **BLK-01** | EXT4 cache ↔ BlockDevice | 读写路径 cache/device **锁序交叉 AB-BA** | **当前实现无 AB-BA**（见 §5.3 复核）；`lock-issues.md` 标为暂缓 |

**本轮代码修复（procfs）**：`procfs-impl/impl-kernel/src/lib.rs` 中 `argv_for` / `exe_for` / `mount_lines` 改为锁内取函数指针、锁外调用。

---

## 1. 概述

本组四个全局结构均位于文件系统辅助层，保护 devfs 设备枚举、procfs 回调注册表，以及 ext4 RW 路径的小块读缓存。锁类型统一为 `spin::Mutex`，通过 RAII guard 自动释锁，**未发现显式漏释锁或重复释锁**；主要风险集中在**持锁区间过长**、**持锁期间堆分配/嵌套驱动锁**，以及（已修复的）**Procfs 持锁调回调**。

| # | 名称 | 文件 | 锁类型 | 保护内容 |
|---|------|------|--------|----------|
| 18 | `DEVFS` | `os/components/wateros-fs/fs-devfs/devfs-impl/impl-kernel/src/lib.rs` | `spin::Mutex<DevFsImpl>` | 节点列表、块/字符设备路径绑定、DTB 未支持路径 |
| 19 | `DEV_NODES` | `os/components/wateros-fs/fs-impl/impl-devfs/src/lib.rs` | `spin::Mutex<Vec<DevNode>>` | 测试/简化 devfs 节点快照 |
| 20 | `ARGV_LOOKUP` / `EXE_LOOKUP` / `MOUNT_LOOKUP` | `os/components/wateros-fs/fs-procfs/procfs-impl/impl-kernel/src/lib.rs` | `spin::Mutex` ×3 | procfs 外部回调（argv/exe/挂载表） |
| 21 | `EXT4_SMALL_READ_CACHE` | `os/components/wateros-fs/fs-impl/impl-ext4/src/rw.rs` | `spin::Mutex<SmallReadCache>` | 单槽块级小读缓存（≤64B、单块内） |

---

## 2. DEVFS（`Mutex<DevFsImpl>`）

### 2.1 锁操作调用点

| 函数 | 操作 | 持锁区间 |
|------|------|----------|
| `DevFsManager::refresh` | `lock()` | 清空并重建 `nodes` / `block_bindings` / `character_bindings` 全过程 |
| `set_dt_unsupported_paths` | `lock()` | 赋值 `dt_unsupported_paths` |
| `list_nodes` | `lock()` | `nodes.clone()` |
| `register_block_device` | `lock()` | 查找/更新/插入绑定 |
| `lookup_block_device` | `lock()` | 线性查找 `block_bindings` |
| `register_character_device` | `lock()` | 查找/更新/插入绑定 |
| `lookup_character_device` | `lock()` | 线性查找 `character_bindings` |
| `default_root_block_path` | `lock()` | 扫描 `nodes` |

公开包装函数（`refresh` / `list_nodes` / `lookup_*` 等）均委托上述方法，无额外锁。

### 2.2 主要调用链

```
driver::init_after_boot / fs::init
  └─ devfs::active_impl::refresh()
       └─ DEVFS.lock() ─┬─ block_device_count/at()  → BLOCK_DEVICES.lock()
                        ├─ character_device_count/at/kind_at() → CHARACTER_DEVICES.lock() [+ 字符设备 dev.lock()]
                        ├─ Vec/String 分配（堆）
                        └─ logging::info!()

VFS / rootfs / fd
  └─ lookup_block_device / lookup_character_device / list_nodes / default_root_block_path
       └─ DEVFS.lock()（短临界区，除 list_nodes 克隆）

impl-fs-bridge::FsBridge::list_dev_nodes
  └─ list_nodes() → DEVFS.lock() + clone
```

### 2.3 持锁区间分析

- **闭环**：所有路径均通过 guard drop 释锁；`refresh` 无提前 return 跳过释锁的分支。
- **嵌套锁（refresh 持 DEVFS 期间）**：
  - `block_device_at` / `block_device_count` → `BLOCK_DEVICES`（`driver-block/block-api`）
  - `character_device_at` / `character_device_kind_at` → `CHARACTER_DEVICES`，且 `kind_at` 还会 `dev.lock()`
  - 锁顺序固定为 **DEVFS → BLOCK_DEVICES / CHARACTER_DEVICES → 单设备 Mutex**，未见反向路径，**当前无 AB-BA 死锁链**。
- **持锁期间堆分配**：`refresh` 与 `list_nodes` 在持 DEVFS 时执行 `Vec::push`、`String::format`、`clone()`，可能触发 `LockedHeap` 自旋锁；若堆分配慢或与其他持锁路径交叉，会放大自旋等待。
- **refresh 语义窗口**：`refresh` 先 `clear()` 绑定表再重建；持锁期间并发 `lookup_*` 被阻塞，释锁前 lookup 可能看到空表——**非数据竞争（有锁）但存在短暂 NotFound 窗口**。
- **`set_dt_unsupported_paths` 与 `refresh` 分离**：两次独立加锁；驱动 `sync_devfs` 顺序调用二者，正常路径一致。

### 2.4 潜在问题

| 严重度 | 问题 | 说明 |
|--------|------|------|
| **P0** | `refresh` 长时间持 DEVFS 自旋锁 | 临界区含驱动枚举、多次嵌套锁、堆分配与日志；单核抢占下若任务持锁被切换，其他任务访问 devfs 将**永久自旋** |
| **P1** | `list_nodes` 持锁 clone 整表 | VFS `list_dev_nodes` 路径可能频繁调用；克隆越大持锁越久 |
| **低** | `refresh()` 包装函数两次加锁 | `refresh()` 后 `list_nodes()` 再次 lock，中间表可能变化（仅影响返回值计数，启动期可忽略） |
| **低** | 未使用 `register_block_device` API | 驱动直接向 `BLOCK_DEVICES` 注册，devfs 靠 `refresh` 同步；动态 register API 与 refresh 竞态未在主路径暴露 |

### 2.5 当前支持范围

| 路径 | 状态 |
|------|------|
| 启动期 `fs::init` / `driver init_after_boot` 单次 `refresh` | 已覆盖，通常无并发 |
| 运行时 `lookup_block/character_device`（open/mknod/mount） | 已加锁，短临界区 |
| 运行时 `list_nodes`（readdir /dev） | 已加锁；大表 clone 有性能/自旋风险 |
| 运行时并发 `refresh` + lookup | **未可靠支持**（长临界区 + 空表窗口） |
| 多核 | 同 spin 语义；`refresh` 与 lookup 互斥正确，但长临界区放大争用 |

### 2.6 收敛建议

1. **`refresh` 拆分临界区**：持锁仅做“交换指针/替换 `DevFsImpl` 快照”，枚举与分配移到锁外，最后 `swap` 一次（copy-on-write 或双缓冲）。
2. **运行时 `refresh`**：若检测到已有用户任务，打印 warn 并拒绝或降级为增量更新：
   ```text
   logging::warn!("[lock] DEVFS: refresh rejected while concurrent accessors may exist (fs-devfs/impl-kernel refresh)");
   ```
3. **`list_nodes`**：改为返回 `Arc<[DevNode]>` 快照指针，读侧无 clone；或只读 `RwLock`（若后续引入）。

---

## 3. DEV_NODES（`Mutex<Vec<DevNode>>`）

### 3.1 锁操作调用点

| 函数 | 操作 | 持锁区间 |
|------|------|----------|
| `refresh` | `lock()` | `clear` + 重建节点 + `trace` 日志 |
| `list_nodes` | `lock()` | `clone()` 整表 |

`lookup_block_device` / `default_root_block_path` **不访问 DEV_NODES**（直接查 `block_device_at` 或 `block_device_count`）。

### 3.2 调用链

```
测试 / 非 kernel devfs 构建
  └─ refresh() → DEV_NODES.lock()
  └─ list_nodes() → DEV_NODES.lock().clone()
```

### 3.3 分析

- 持锁闭环正确；`refresh` 同样嵌套 `block_device_count()` → `BLOCK_DEVICES` 短锁。
- **无字符设备、无动态 register**；临界区比 DEVFS 短，但仍含堆分配。
- `lookup_block_device` 不读 DEV_NODES：refresh 与 lookup **无锁耦合**；索引靠路径解析 + 驱动表，**节点表 stale 不影响 lookup 正确性**（仅 `list_nodes` 可能过时）。

### 3.4 潜在问题

| 严重度 | 问题 | 说明 |
|--------|------|------|
| **P1** | `refresh` 持锁期间堆分配 | 与 DEVFS 同类，但范围更小；测试环境多线程 refresh 仍可能自旋卡死 |
| **低** | 节点表与 lookup 语义分离 | `list_nodes` 与 `lookup` 不一致是设计选择，非锁缺陷 |

### 3.5 当前支持范围

| 路径 | 状态 |
|------|------|
| bring-up / 单元测试单次 refresh | 已覆盖 |
| 并发 refresh + list_nodes | 互斥正确，但可能长时间自旋 |
| 生产 kernel 路径 | 使用 `impl-kernel` DEVFS，**本结构不在 kernel 路径** |

### 3.6 收敛建议

- 测试路径保持现状即可；若 CI 并行跑多测试线程访问 devfs，对 `refresh` 加 warn 或串行化测试 fixture。
- 与 DEVFS 对齐：锁外构建 `Vec` 再一次性 replace。

---

## 4. ProcfsLookups（`ARGV_LOOKUP` / `EXE_LOOKUP` / `MOUNT_LOOKUP`）

### 4.1 锁外调用修复（PROC-01，已落地）

**修复前**：`argv_for` 等在 `ARGV_LOOKUP.lock().and_then(|f| f(leader))` 内持锁执行回调。  
**修复后**（`procfs-impl/impl-kernel/src/lib.rs:37–50`）：

```rust
fn argv_for(leader: TaskId) -> Option<Vec<String>> {
    let lookup = *ARGV_LOOKUP.lock();
    lookup.and_then(|f| f(leader))
}
```

`exe_for`、`mount_lines` 同理：锁内仅复制 `Option<fn>`（`Copy` 函数指针），**释锁后再调用**。  
`TaskArgvLookup` / `TaskExeLookup` / `MountListLookup` 为 `fn` 类型别名，复制不触发堆分配。

**效果**：

- 消除「持 lookup 锁等 cwd/VFS」导致的**永久自旋**（PROC-01 主因）。
- 消除「回调重入 procfs → 二次 `ARGV_LOOKUP.lock()`」导致的**同锁自死锁**。
- `mount_lines` 回调内 `list_proc_mount_lines` 再锁 `AUX_MOUNTS` / `ROOT_RW_FS` 时，**不再嵌套在 MOUNT_LOOKUP 下**。

### 4.2 锁操作调用点（当前）

| 静态量 | 注册（写） | 读侧 |
|--------|-----------|------|
| `ARGV_LOOKUP` | `register_task_argv_lookup` → `*lock() = Some(f)` | `argv_for` → 锁内 `*lock()`，锁外 `f(leader)` |
| `EXE_LOOKUP` | `register_task_exe_lookup` | `exe_for` → 同上 |
| `MOUNT_LOOKUP` | `register_mount_list_lookup` | `mount_lines` → 锁内 `*lock()`，锁外 `f()` |

读侧消费点：

- `comm_for` → `argv_for` + `exe_for`（两次独立加锁，中间无 lookup 锁）
- `format_cmdline` → `argv_for` + `exe_for`
- `format_mounts` → `mount_lines`
- 经 `KernelProcFs::{read, metadata, exists, read_dir}` → VFS `proc_handle::open_proc`

注册时机：`vfs::mount_procfs_at`（挂载 `/proc` 前一次性注册三个回调）。

### 4.3 回调实际行为（锁外执行）

| 回调 | 注册闭包 | 回调内触发的锁 |
|------|----------|----------------|
| argv | `cwd::lookup_argv_for_task` | `UniprocessorSafeCell<PerTaskCwdRegistry>::exclusive_access()` |
| exe | `cwd::lookup_exe_for_task` | 同上 |
| mount | `impl_fs_bridge::list_proc_mount_lines` | `ROOT_RW_FS.lock()`（clone）+ `AUX_MOUNTS.lock()`（迭代）；`default_root_block_path` → `DEVFS.lock()` |

### 4.4 持锁区间分析（修复后）

- **lookup 锁临界区**：单次 `*lock()` 复制函数指针，极短；**闭环正确**。
- **剩余风险（PROC-02）**：cwd 回调仍用 `UniprocessorSafeCell`；若持 `exclusive_access` 时被抢占，其他任务访问 cwd 注册表 → `RefCell already borrowed` panic（RC-2，见 `per-task-registries` 审计）。
- **procfs 重入**：lookup 锁已释放；若回调内再读 `/proc/...`，不会自旋死锁于 lookup，但仍可能与其他 VFS/procfs 路径交错（低概率，文档约束即可）。
- **锁顺序（mount 回调）**：`ROOT_RW_FS` → `AUX_MOUNTS` →（可选）`DEVFS`；挂载变更路径持 `AUX_MOUNTS` 但不读 proc；**无 AB-BA**。
- **注册函数无读侧协调**：运行时 `register_*` 覆盖回调与并发读可能短暂看到 `None`/旧 fn（挂载后通常不变）。

### 4.5 潜在问题

| 严重度 | 问题 | 说明 |
|--------|------|------|
| ~~**P0**~~ | ~~持 lookup 自旋锁执行 VFS/cwd 回调~~ | **已修复**（§4.1） |
| **P1** | cwd 回调 `UniprocessorSafeCell` | 抢占下 panic；与 procfs 已解耦，属 per-task 注册表问题 |
| **低** | `comm_for` 两次加锁 argv/exe | 非死锁；中间 argv 表可能变化导致 comm 不一致 |
| **低** | 运行时重新 `register_*` | 无与读侧协调；挂载后不应发生 |

### 4.6 当前支持范围

| 路径 | 状态 |
|------|------|
| 挂载后读 `/proc/meminfo`、`/proc/sys/*`（不触发 lookup） | 安全 |
| 读 `/proc/<pid>/cmdline`、`status`、`stat`（触发 argv/exe） | lookup 锁语义**已可靠**；cwd 仍受 RC-2 约束 |
| 读 `/proc/mounts` | lookup 锁外调 `list_proc_mount_lines`；嵌套 `AUX_MOUNTS` 正常 |
| 运行时重新 `register_*` | 未定义行为，无锁保护 |

### 4.7 收敛建议（剩余）

1. cwd 注册表迁移至 `spin::Mutex` 或与 RC-2 统一方案（见 `per-task-registries`）。
2. debug 下检测 procfs 重入深度并 warn（非必须，lookup 已不持锁）。
3. 未注册回调遇 `None` 时可显式 warn（当前 `mount_lines` 用 `unwrap_or_default`）。

---

## 5. EXT4_SMALL_READ_CACHE

### 5.1 锁操作调用点

| 函数 | 操作 | 说明 |
|------|------|------|
| `read_with_small_cache` | `lock()` ×2（命中 1 次，未命中 2 次） | 检查缓存 / 填充缓存 |
| `read_with_small_cache`（大读/跨块） | 仅 `dev.lock()` | 绕过缓存 |
| `invalidate_small_read_cache` | `lock()` ×1–3 | 写路径前置失效 |
| `block_write_bytes` | 先 `invalidate_*`，再 `dev.lock()` | 写路径 |

仅 **RW ext4**（`rw.rs` `BlockDevRw`）使用；**RO 路径**（`ro.rs`）直接 `dev.lock().read_bytes`，**不经过本缓存**。

### 5.2 调用链

```
Ext4FsRw / SharedRwFs 操作
  └─ ext4plus 读
       └─ BlockDevRw::read → read_with_small_cache
            ├─ EXT4_SMALL_READ_CACHE.lock()  [查，释锁]
            ├─ SharedBlockDevice.lock()      [读块，释锁]
            └─ EXT4_SMALL_READ_CACHE.lock()  [填]

BlockDevRw::write → block_write_bytes
  └─ invalidate_small_read_cache → EXT4_SMALL_READ_CACHE.lock() [释锁]
  └─ dev.lock() → write_blocks
```

上层通常还持有 `Arc<Mutex<LocalRwFs>>`（SharedRwFs 实例锁），顺序为：**FS 实例锁 → EXT4_SMALL_READ_CACHE → 块设备锁**（三层不同时嵌套 FS 锁与 cache 锁的情况取决于 ext4plus 回调是否仍持 FS 锁——bridge 层在 `read_range` 内短持 FS 锁后调用 ext4）。

### 5.3 锁顺序复核（BLK-01 / RC-5）

**结论：当前 `rw.rs` 实现不存在 cache ↔ device 同时持有导致的 AB-BA 死锁。**

| 路径 | 实际锁序 | 是否同时持两把锁 |
|------|----------|------------------|
| 小读命中 | 仅 `EXT4_SMALL_READ_CACHE` | 否 |
| 小读未命中 | cache(查) → **释** → dev(读块) → **释** → cache(填) | **否**（L68–86 显式分段） |
| 大读/跨块 | 仅 `dev.lock()` | 否 |
| 写 | cache(invalidate) → **释** → `dev.lock()` | **否** |

**AB-BA 必要条件**（线程 A 持 dev 等 cache，线程 B 持 cache 等 dev）在当前代码中**不可达**：

- 未命中读在 `dev.lock()` 返回后**才**第二次取 cache 锁（L76–81）。
- `invalidate_small_read_cache` 在 `block_write_bytes` 取 dev 锁**之前**完成并释锁（L164–165）。
- 写路径顺序为 cache → dev，读未命中为 cache → dev → cache，**任意时刻最多持有一把锁**。

`driver-block-char.md` §5.1 所述「写 path cache→device 与读 path device→cache 交叉」描述的是**锁获取先后顺序不同**，并非**嵌套持锁**；在「不同时持有两把锁」前提下不构成经典 AB-BA。多任务并发时仅为**互斥等待**（争用），非死锁。

**与 `CachingBlockDevice`（feature `block-cache`）**：外层 `SharedBlockDevice` Mutex 仍覆盖 LRU + VirtIO I/O 全长；ext4 小读缓存在其**之上**再加一层全局 Mutex，放大争用（P1），但不引入额外 AB-BA（ext4 先释 dev 再动 cache）。

**数据一致性**：未命中 TOCTOU 窗口（释 cache → 读盘 → 填 cache）中，写者写前 `invalidate`，读者随后读盘拿新数据——单写者场景一致；极端并发可能重复读盘（正确性 OK，性能浪费）。

### 5.4 潜在问题

| 严重度 | 问题 | 说明 |
|--------|------|------|
| ~~**P0**~~ | ~~cache/device AB-BA~~ | **当前实现无**（§5.3） |
| **P1** | 全局单 Mutex 争用 | 高频小读 ext4 元数据时多任务自旋；单核抢占下持锁被切换会阻塞所有 ext4 读者 |
| **低** | 全局单槽 | 多文件/多块并发读互相挤占，仅性能 |
| **低** | `dev_id = Arc` 指针地址 | 同一 `SharedBlockDevice` 共享键；符合预期 |
| **低** | RO/RW 混用 | RO 不经过缓存；RW 写 invalidate；无 stale 泄漏到 RO |

### 5.5 当前支持范围

| 路径 | 状态 |
|------|------|
| 单任务 ext4 RW bring-up / 小文件测试 | 已覆盖 |
| 多任务并发读同一 ext4 卷 | 互斥正确，可能自旋争用 |
| 多 ext4 实例 / 多设备 | dev_id 区分，行为正确 |
| RO ext4 挂载 | 不使用本缓存 |
| 多核 | `spin::Mutex` 互斥；单槽非 per-CPU |

### 5.6 收敛建议

1. 高并发：**per-device 缓存**（每 `SharedBlockDevice` 挂 `Mutex<SmallReadCache>`）或移入 `CachingBlockDevice` 层，降低全局争用（P1，非紧急）。
2. 若未来改为「持 dev 锁期间更新 cache」，**必须**固定全局顺序（先 cache 后 dev，或合并为单锁），并回归 ext4 测例——当前无需为此改热路径。
3. 写路径保持 **写前 invalidate** 顺序。

---

## 6. 跨结构锁顺序小结

```
DEVFS ──→ BLOCK_DEVICES / CHARACTER_DEVICES ──→ 单块设备 Mutex

MOUNT_LOOKUP（仅注册表短锁）── 锁外 ──→ ROOT_RW_FS ──→ AUX_MOUNTS ──→ DEVFS
ARGV/EXE_LOOKUP（仅注册表短锁）── 锁外 ──→ UniprocessorSafeCell (PerTaskCwdRegistry)

SharedRwFs Mutex ──→ EXT4_SMALL_READ_CACHE ──→ SharedBlockDevice Mutex
                     （分段：不同时持 cache+dev）
```

**未见** DEVFS ↔ ProcfsLookups ↔ EXT4_CACHE 之间的直接嵌套；Procfs lookup 持锁调回调问题**已消除**。

---

## 7. 高优先级修复列表（当前 Top 3）

| 优先级 | 结构 | 问题 | 后果 | 建议 | 状态 |
|--------|------|------|------|------|------|
| ~~P0~~ | ProcfsLookups | 持 Mutex 执行 cwd/VFS 回调 | 自旋死锁 / 永久等待 | 锁内复制 fn，锁外调用 | **已修复** |
| **P0** | `DEVFS` | `refresh` 长临界区 | 运行时 refresh 或并发 dev 访问长时间自旋；lookup 空窗 | 锁外构建快照再 swap；运行时 warn + 拒绝 | 待修 |
| **P1** | `EXT4_SMALL_READ_CACHE` | 全局单锁三层嵌套争用 | 多任务 ext4 小读自旋 | per-device 缓存或缩短临界区 | 待修 |

---

## 8. 审计结论

- **释锁闭环**：四结构均依赖 RAII，无显式漏释锁。
- **已修复**：ProcfsLookups 锁外调回调（PROC-01）；与 `lock-issues.md` 第 13 项一致。
- **EXT4 锁序**：复核确认**无 cache/device AB-BA**；BLK-01 暂缓理由为「统一锁序需改热路径」，当前实现已通过分段释锁规避死锁，剩余为 **P1 争用**。
- **主要未决缺陷**：`DEVFS.refresh` 长临界区（P0）；`DEV_NODES` 测试路径同类问题（P1，非 kernel）；procfs cwd 回调仍受 RC-2 影响（P1）。
- **建议修复顺序**：DEVFS refresh 拆分 → EXT4 per-device 缓存（性能）→ cwd 注册表 RC-2 收敛。

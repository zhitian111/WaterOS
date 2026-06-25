# 锁机制审计：`KLOG` / `KlogRingbufInner`

> 审计日期：2026-06-25（复审计：InterruptGuard 修复验证）  
> Baseline：单核多线程、可抢占调度（timer tick → `task::schedule_tick`）  
> 清单编号：lock-inventory #35

## 0. P0 / P1 / 已修复摘要

| ID | 严重度 | 问题 | 状态 |
|----|--------|------|------|
| **KLOG-01** / §4.1 | **P0** | 可抢占 + `spin::Mutex` → 持锁任务被切换后其他任务永久自旋 | **已修复** — `KlogInterruptGuard` 包裹 `with` / `iter_from` |
| **KLOG-02** / §4.2 | **P0** | 任务持锁时被 IRQ 打断，IRQ 路径再 `KLOG.lock()` 永久自旋 | **已修复** — 持锁前关全局中断，timer/IRQ 无法在临界区内插入 |
| **KLOG-03** / §4.3 | **P1** | `with` / `iter_from` 闭包内递归 `record` / `klog_*!` 不可重入 | **未修复** — API 契约 + 无运行时检测 |
| §4.4 | P2 | 首次 `ensure_inner` 锁内 ~270 KiB `Default` | **未修复** — 窗口已缩短（关中断），仍建议 eager init |
| §4.5 | P3 | `read_all` 多次 `with` 读批撕裂 | **未修复** — 语义偏差，非死锁 |

**修复实现**（与帧分配器 `FrameAllocatorInterruptGuard`、调度器 `InterruptGuard` 同模式）：

```253:303:os/components/wateros-klog/klog-impl/klog-ringbuf/src/lib.rs
struct KlogInterruptGuard {
    state : ArchInterruptState,
}
// ... new / Drop: read → disable_global_interrupt → restore on drop

impl KlogRingbuf {
    pub fn with<R>(f: impl FnOnce(&mut KlogRingbufInner) -> R) -> R {
        let _irq = KlogInterruptGuard::new();
        let mut guard = KLOG.lock();
        f(ensure_inner(&mut guard))
    }

    pub fn iter_from<F>(start_seq: u64, mut f: F) { /* 同上 */ }
}
```

- **获取顺序**：关中断 → `KLOG.lock()`（正确：自旋前禁止抢占源）
- **释放顺序**：`MutexGuard` drop → `KlogInterruptGuard` drop（Rust 逆序 drop：先释锁再恢复中断）
- **未覆盖路径**：`KlogRingbuf::init()` 仍直接 `KLOG.lock()`，**无** `KlogInterruptGuard`；当前仅 `kernel_main` 在 `run_first_task` 前调用，baseline 下可接受

---

## 1. 概述

| 项 | 内容 |
|---|---|
| 数据结构 | 全局内核消息环 `KLOG: Mutex<Option<KlogRingbufInner>>` |
| 主要文件 | `os/components/wateros-klog/klog-impl/klog-ringbuf/src/lib.rs` |
| 锁类型 | `spin::Mutex` + **`KlogInterruptGuard`**（`with` / `iter_from` 运行时路径） |
| 聚合 API | `os/components/wateros-klog/src/lib.rs`（`record` / `klog_*!`） |
| Syscall 路径 | `os/components/wateros-klog/src/syscall.rs` ← `sys_syslog` |
| 预估复杂度 | 中（全局单锁 + 多入口；P0 抢占死锁已收敛） |

`KlogRingbufInner` 为静态大结构（256 槽 × 每槽 ~1 KiB 正文 ≈ **270 KiB BSS**），`ensure_inner` 在首次访问时用 `Default` 填充，**无堆分配**。

---

## 2. 加锁 / 释锁调用点

### 2.1 底层（唯一 mutex）

| 函数 | 文件:行 | 操作 | 中断保护 | 释锁方式 |
|------|---------|------|----------|----------|
| `KlogRingbuf::init` | `klog-ringbuf/src/lib.rs:283` | `KLOG.lock()` | ❌（boot 专用） | guard Drop |
| `KlogRingbuf::with` | 同上:290–292 | `KlogInterruptGuard` → `KLOG.lock()` | ✅ | guard Drop（先 mutex 后 irq） |
| `KlogRingbuf::iter_from` | 同上:300–302 | 同上 | ✅ | 同上 |

所有路径均通过 RAII 释锁，**未发现漏释锁 / 重复释锁**。

### 2.2 经 `with` / `iter_from` 间接加锁的入口

| 调用方 | 文件 | 持锁操作 | 中断保护 |
|--------|------|----------|----------|
| `klog::init` | `klog/src/lib.rs:17` | `init()` → `reset()` | ❌（经 `init`，非 `with`） |
| `klog::record_with_meta` | 同上:40 | `append` | ✅ |
| `klog::stats` | 同上:45 | `stats()` | ✅ |
| `klog::iter_from` | 同上:53 | 迭代闭包（**持锁贯穿整个 `f`**） | ✅ |
| `dispatch_kernel` SIZE/CLEAR | `klog/src/syscall.rs:23–27` | 统计 / 清游标 | ✅ |
| `read_one` | 同上:40 | `peek` + 可选 `advance_read_cursor` + `format_traditional` | ✅ |
| `read_all` | 同上:61（循环） | 每条记录一次 `with` | ✅（单次持锁短） |
| `write_priority` | 同上:95 | 经 `record_with_meta` → `append` | ✅ |
| 单元测试 | `klog-ringbuf/src/lib.rs:315` | 测试闭包 | ✅ |

### 2.3 当前实际调用面（代码库扫描）

| 路径 | 状态 |
|------|------|
| `os/src/main.rs`：`klog::init` / `post_init_hello` | 已用（**调度启动前**，无抢占） |
| `sys_syslog` → `dispatch_kernel` | 已用（用户任务内核态，**经 InterruptGuard 保护**） |
| `klog_*!` 宏 | **已实现，全库零调用** |
| `klog::iter_from` / `stats` | 仅文档/API，无运行时调用方 |
| `runtime::logging`（`log::info!` 等） | **独立通道**（控制台），不经过 `KLOG` |

---

## 3. 持锁区间分析

### 3.1 写入路径（`append`）

```
record / klog_*! / write_priority
  → [锁外] ts_nsec_now(), caller_id_now(), 宏内 KlogFmtBuffer 格式化
  → KlogInterruptGuard::new()
  → KLOG.lock()
  → ensure_inner（首次：~270 KiB Default 初始化）
  → append：槽扫描、copy_from_slice、refresh_oldest_seq（O(slots)）
  → MutexGuard Drop → KlogInterruptGuard Drop
```

- **锁外格式化**：设计合理。
- **`caller_id_now`**：经 `task::current_task_id()`（内部调度器 `InterruptGuard` + `UniprocessorSafeCell`），在 **KLOG 锁外**调用，无与 KLOG 的直接锁顺序问题。
- **持锁期间无堆分配**：仅栈上/meta 拷贝与固定数组写入。

### 3.2 读取路径（syslog READ）

```
sys_syslog → dispatch_kernel → read_one / read_all
  → KlogInterruptGuard + KLOG.lock()
      → peek_next_unread（O(slots) 扫描）
      → [可选] advance_read_cursor
      → format_traditional（栈上 line 缓冲，在 with 闭包内）
  → [锁外] copy_to_user
```

- `read_all`：**每条记录单独加锁**；两次 `with` 之间可被写入/覆盖（§4.5 语义偏差，非死锁）。

### 3.3 `iter_from` 持锁范围

`KlogRingbuf::iter_from` 在 **整个用户闭包 `f` 执行期间持锁**（且关中断）。当前无调用方；若未来在 `f` 内打日志 → §4.3 自旋；若 `f` 阻塞 → 关中断时间过长（当前无阻塞 API）。

### 3.4 与调度 / 中断的交互（修复后）

```
用户/内核任务: KlogInterruptGuard（关中断）→ KLOG.lock() 持有中
    ↓ timer IRQ
    ✗ 无法投递（SIE 关）→ schedule_tick 不运行 → 不会被 __switch 抢占
    ↓ guard Drop
    恢复中断 → 其他任务可正常 lock / 自旋完成
```

- **KLOG-01 根因已消除**：单核 baseline 下，关中断临界区等价于「禁止 timer 抢占持锁任务」。
- **KLOG-02 根因已消除**：持锁期间 IRQ 不可重入抢锁；IRQ 上下文调用 `with` 时若锁空闲则正常，与同线程二次 `lock`（§4.3）是唯一剩余自旋风险。
- trap 路径当前使用 `log::trace!`（控制台），**未**调用 klog；修复后即使未来误用，与「任务持锁 + IRQ 互斥」经典死锁链已断开。

---

## 4. 潜在问题（按严重程度）

### 4.1 ~~严重~~ — 可抢占 + 自旋锁 → 卡死或长时间活锁 — **已修复（KLOG-01）**

**原现象**：任务 A 持 `KLOG` 时被 timer 抢占；任务 B `sys_syslog` 永久自旋。

**修复**：`with` / `iter_from` 在 `lock()` 前 `disable_global_interrupt`，与 RC-3 收敛策略一致（见 `lock-issues.md` KLOG-01）。

**验证建议**：两用户任务并发 syslog WRITE + READ 压力测试（应不再 hang）。

### 4.2 ~~严重~~ — 中断 / trap 上下文调用 klog 与持锁任务互斥 — **已修复（KLOG-02）**

**原现象**：任务 A 持锁时被 IRQ 打断；IRQ 内 `KLOG.lock()` 永久自旋。

**修复后**：持锁路径关中断，上述交错不可发生。仍建议在 `record_with_meta` 增加 trap 上下文检测 + warn（防御性，非 P0）。

### 4.3 高 — 非可重入：`with` / `iter_from` 闭包内递归日志 — **P1 未修复（KLOG-03）**

**现象**：闭包内调用 `record` / `record_with_meta` / `klog_*!` / `write_priority` → 同线程二次 `KLOG.lock()` → **永久自旋**（`spin::Mutex` 不可重入；外层已关中断，无法被抢占解脱）。

**当前状态**：现有闭包体未递归打日志；`iter_from` 无调用方。随 `klog_*!` 推广极易踩坑。

### 4.4 中 — 首次 `ensure_inner` 持锁初始化大结构 — **P2 未修复**

**现象**：首次 `with`/`init` 时在锁内执行 `KlogRingbufInner::default()`（~270 KiB 清零），关中断窗口长于常规 `append`。

**影响**：不再导致死锁，但延长关中断时间；首条 syslog 前若竞争仍影响延迟。

### 4.5 中 — `read_all` 细粒度加锁导致可见性撕裂 — **P3 未修复**

**现象**：循环内多次 `with`，两次读之间记录可被覆盖或插入。

**影响**：数据语义偏差；单核下不致死锁。

### 4.6 低 — 与 `runtime::logging` 双轨

**现象**：内核 `log::info!` 走控制台，`klog_*!` 走环缓冲，二者无锁耦合。

**影响**：非锁 bug；审计「持锁打 log」时需区分通道。

---

## 5. 当前实际支持范围

| 场景 | 是否可靠 | 说明 |
|------|----------|------|
| 引导阶段、`run_first_task` 前 `init` / `post_init_hello` | ✅ | 无抢占、单线程 |
| 用户态 `syslog(2)` READ/WRITE（调度已启动） | ✅ | `KlogInterruptGuard` 保护（KLOG-01 已修复） |
| 多任务并发 `sys_syslog` | ✅ | 互斥正确；高争用仍自旋但可完成 |
| IRQ / trap 上下文 `klog::record` | ⚠️ | 经典 IRQ↔任务死锁已修复；同线程重入仍死锁（§4.3） |
| `with` / `iter_from` 闭包内再写 klog | ❌ | 非可重入（§4.3 / KLOG-03） |
| `klog_*!` 宏（任意内核路径） | ⚠️ | 已实现未使用；启用后等同 `record`，需遵守 §4.3 |
| 持锁期间堆分配 | ✅ | 无堆分配 |
| 锁成对释放 | ✅ | RAII 闭环 |
| `KlogRingbuf::init` 运行期重入 | ⚠️ | 无 InterruptGuard；当前仅 boot 调用 |

---

## 6. 收敛建议

### 6.1 ~~持锁临界区（#4.1）~~ — **已完成**

`KlogInterruptGuard` 已落地于 `with` / `iter_from`。可选：`init()` 同样包裹 guard 以统一契约。

### 6.2 禁止中断上下文写入（#4.2）— **机制已满足，可选加强**

临界区关中断已消除 P0 死锁链。可选在 `record_with_meta` 入口检测 trap/IRQ 上下文并 warn + 丢弃（防御误用）。

建议 warn 格式：

```text
[klog] KLOG spin lock: append rejected in interrupt context at klog/src/lib.rs:record_with_meta (level={}, len={})
```

### 6.3 禁止闭包内递归 klog（#4.3）— **待做（P1）**

- **文档**：在 `KlogRingbuf::with`、`iter_from` rustdoc 标明「闭包内不得调用任何 klog 写入 API」。
- **调试构建**：线程局部 `KLOG_LOCK_HELD` 标志，`record_with_meta` 若已置位则 warn + 丢弃。

### 6.4 缩短持锁时间（#4.4）— **待做（P2）**

- 将 `ensure_inner` 提前到 `klog::init`（eager 填充），避免首次 syscall 在锁内 `Default` 大结构。
- `read_one`：可将 `format_traditional` 移到锁外（锁内仅 `peek` + 拷贝 meta/text 到栈缓冲）。

### 6.5 `read_all`（#4.5，可选 P3）

- 单次 `with` 内批量 peek/advance，或接受「best-effort 批量读」语义并在文档 B 标注与 Linux 差异。

---

## 7. 锁顺序与交叉引用

| 关联 | 关系 |
|------|------|
| `task::current_task_id` | 在 KLOG **锁外**调用；顺序：`InterruptGuard`（task）→ 释放 → `KlogInterruptGuard` → `KLOG.lock()` |
| `runtime::logging` / `console` | 无共享锁 |
| 其他 `spin::Mutex`（VFS、IPC 等） | **无固定全局顺序**；若「持其他 spin 锁 → klog」与「持 klog → 其他锁」并存，可能死锁；随 `klog_*!` 推广需纳入全局 lock ordering |
| RC-3（`lock-issues.md`） | klog 为已修复实例；同类风险仍存在于 network、block dev 等 |

---

## 8. 测试建议

1. 两用户任务并发 `syslog` WRITE + READ 压力（调度已开），确认修复后无 hang。
2. 一任务 `READ_ALL` 循环 + 另一任务高频 WRITE，验证 §4.5 语义与性能。
3. （P1）在 `with` 闭包内故意 `record` 的 debug 断言 / warn 触发测试。

---

## 9. 高优先级修复列表（本结构）

| 优先级 | 问题 | 建议动作 | 状态 |
|--------|------|----------|------|
| ~~P0~~ | KLOG-01 / §4.1 可抢占下 spin 锁卡死 | `with` 持锁区关中断 | **已修复** |
| ~~P0~~ | KLOG-02 / §4.2 IRQ 上下文 klog | 关中断临界区 | **已修复** |
| **P1** | KLOG-03 / §4.3 闭包递归日志 | 文档 + debug 重入检测 | 待做 |
| P2 | §4.4 首次大结构持锁 init | `klog::init` eager 初始化 | 待做 |
| P3 | §4.5 read_all 撕裂 | 批量读或文档标注 | 待做 |

# 锁机制审计：StackFrameAllocator + InterruptSafeLockedHeap

> **Subagent 分组**：`mm-allocators`（清单 #8–#9）  
> **审计日期**：2026-06-25（**复核**：2026-06-25，含 InterruptGuard 修复验证）  
> **Baseline**：单核多线程；定时器中断可触发抢占式 `schedule_tick`  
> **主要文件**：
> - `os/components/wateros-mm/mm-frame-alloctor/frame-alloctor-impl/impl-stack/src/lib.rs`
> - `os/components/wateros-runtime/runtime-heap-allocator/src/lib.rs`
> - 原语：`os/components/wateros-base/src/sync/uniprocessor.rs`

---

## 1. 概述

本组覆盖内核两类全局分配器：

| # | 数据结构 | 锁/同步原语 | 角色 |
|---|---------|------------|------|
| 8 | `StackFrameAllocator`（全局单例 `FRAME_ALLOCATOR`） | `UniprocessorSafeCell`（内部 `RefCell`）+ **`FrameAllocatorInterruptGuard`** | 物理页帧（PPN）分配/回收 |
| 9 | `InterruptSafeLockedHeap`（`HEAP_ALLOCATOR`） | `spin::Mutex`（`linked_list_allocator::LockedHeap` 内部）+ 中断屏蔽守卫 | 内核全局堆 `GlobalAlloc` |

两者均为**全局单例**，被 MM 页表、syscall（mmap/brk/shm）、驱动（VirtIO DMA）、运行时 `alloc` 等广泛调用。

---

## 2. StackFrameAllocator

### 2.1 数据结构与锁类型

```rust
static mut FRAME_ALLOCATOR: MaybeUninit<UniprocessorSafeCell<StackFrameAllocator>>;
static FRAME_ALLOCATOR_READY: AtomicBool;
```

- **锁类型**：`UniprocessorSafeCell<StackFrameAllocator>` → `RefCell` 运行时借用
- **非**自旋锁；`exclusive_access()` 失败时 **panic**（`RefCell already borrowed`）
- **外层守卫**（2026-06-25 修复）：`FrameAllocatorInterruptGuard` — 构造时读/关全局中断，drop 时恢复；与调度器 `InterruptGuard` 同模式（`expect` 不吞错误）

```rust
struct FrameAllocatorInterruptGuard {
    state: ArchInterruptState,
}

fn with_frame_allocator<R>(f: impl FnOnce(&mut StackFrameAllocator) -> R) -> R {
    let _guard = FrameAllocatorInterruptGuard::new();
    f(&mut get_frame_allocator_cell().exclusive_access())
}
```

**释借顺序**：`f` 返回后 `RefMut` 先 drop，再 drop `_guard` 恢复中断 — 正确。

### 2.2 等价 lock / unlock 调用点

`UniprocessorSafeCell` 无显式 `lock()`；**借出 = 持锁，RefMut drop = 释锁**。所有运行期导出 API 经 `with_frame_allocator` 包装（关中断 + 借出）。

| 函数 | 文件:行 | 同步方式 | 持锁区间 |
|------|---------|---------|---------|
| `init_frame_allocator` | `impl-stack/src/lib.rs:237-245` | `with_frame_allocator` → `init(...)` | `init()` 全程（含 `Vec::resize`） |
| `frame_alloc` | `:254-255` | `with_frame_allocator` → `alloc_frame()` | `alloc_frame` 全程 |
| `frame_dealloc` | `:259-260` | 同上 | `dealloc_frame` 全程（含 `recycled.contains` O(n)） |
| `frame_alloc_result` | `:267-268` | 同上 | 同上 |
| `frame_dealloc_result` | `:271-272` | 同上 | 同上 |
| `frame_inc_ref` | `:275-276` | 同上 | `inc_ref` 全程 |
| `frame_ref_count` | `:279-280` | 同上 | `ref_count` 全程 |
| `frame_mem_stats` | `:291` | 同上 | `mem_stats` 全程 |
| `GlobalPhysFrameAllocator::alloc_frame` | `:306` | 委托 `frame_alloc_result()` | 每次调用独立短借 |
| `GlobalPhysFrameAllocator::dealloc_frame` | `:309-310` | 委托 `frame_dealloc_result()` | 每次调用独立短借 |
| `frame_allocator_cell()` | `:249-250` | **无守卫**；仅返回 `&UniprocessorSafeCell` | 调用方若直接 `exclusive_access()` 则绕过 InterruptGuard |

**间接调用方（节选）**：

| 调用链 | 场景 |
|--------|------|
| `pagetable.rs` → `frame_alloc_result` / `frame_dealloc_result` / `frame_inc_ref` / `frame_ref_count` | COW、fork、destroy、lazy fault |
| `kernel_global.rs` | 内核匿名映射 |
| `user_access.rs` → `GlobalPhysFrameAllocator` → lazy fault | syscall 用户内存拷贝 |
| `syscall/.../mmap.rs`, `brk.rs`, `shm.rs` | 用户 mmap/brk/shm |
| `driver-network/.../virtio-pci/src/lib.rs` | VirtIO DMA 帧分配 |
| `mm/src/lib.rs` | MM 聚合层测试 |
| `main.rs` → `mm::test_with_range` | bring-up 自检 |

### 2.3 InterruptGuard 修复验证（SFA-1）

| 检查项 | 结果 |
|--------|------|
| 所有导出分配/回收/引用 API 经 `with_frame_allocator` | ✅ |
| `read_global_interrupt_state` 失败路径 | ✅ 使用 `expect`，不静默吞掉（优于堆分配器） |
| `disable` / `restore` 失败路径 | ✅ `expect` |
| 与调度器 `InterruptGuard` 嵌套 | ✅ 内层读到的 `SIE` 可能已为 0，恢复后保持外层关中断状态 |
| 与堆 `with_allocator_interrupt_guard` 嵌套（`init`/`dealloc` 中 `Vec` 增长） | ✅ 帧侧已关中断；堆侧再关/恢复，最终帧 guard drop 恢复帧入口快照 |
| 定时器 `schedule_tick` 在持借期间插入 | ✅ 关中断后不会抢占，RefCell 不会被跨任务重入 |
| `frame_allocator_cell().exclusive_access()` 绕过 | ⚠️ API 仍导出；**当前代码库无直接调用方**（grep 仅 impl-stack 自身） |

**结论**：SFA-1（抢占 vs RefCell panic）**已修复**；单核 + 定时器抢占 baseline 下帧分配路径可靠。

### 2.4 持锁区间与闭环分析

**释锁闭环**：各导出函数均为「`FrameAllocatorInterruptGuard` → `exclusive_access()` → 操作 → RefMut 析构 → guard 析构」，无早退漏释。

**持锁期间可能触发的副作用**：

1. **`init()` / `dealloc_frame()` 中的 `Vec` 操作**（`resize`、`push`）可能调用全局堆 `alloc`/`realloc`，此时仍持有 `RefMut` + 关中断。
2. **`dealloc_frame()` 中 `recycled.contains(&frame)`** 为 O(n) 扫描，持借时间随回收栈深度线性增长。
3. **`log::warn!`** 在错误路径打印，经 `runtime-logging` → `console::println`，当前实现不经过堆分配。

**嵌套重入**：

- 文档已警告（`:296-299`）：不得在持有 `frame_allocator_cell().exclusive_access()` 期间嵌套调用 `frame_alloc_result`。
- 当前代码库**无**直接 `frame_allocator_cell().exclusive_access()` 的长持有调用方。
- **隐式嵌套**：`pagetable.rs` 中 COW 连续调用 `frame_ref_count` → `frame_alloc_result` → `frame_dealloc_result`，每次独立 `with_frame_allocator` 调用，**无** RefCell 嵌套。

### 2.5 潜在问题

| ID | 严重程度 | 类型 | 描述 | 状态 |
|----|---------|------|------|------|
| SFA-1 | ~~P0 卡死/Panic~~ | 抢占 vs RefCell | 帧分配路径未关中断 → 定时器抢占 → `RefCell already borrowed` panic | **已修复**（`FrameAllocatorInterruptGuard` + `with_frame_allocator`） |
| SFA-2 | **P1** | 持借期间堆分配 | `init()`、`dealloc_frame()` 的 `Vec` 增长在持有 `RefMut`+关中断时走 `GlobalAlloc`；持借窗口被拉长；若未来堆路径回调帧分配将形成 RefCell 重入 | 开放 |
| SFA-3 | **P1 语义/损坏** | 非锁但相关 | `init_frame_allocator()` 可重复调用并重置池，不校验已分配帧；`test_with_range` 末尾二次 `init` 可接受，运行期误调将导致双重释放/帧池损坏 | 开放 |
| SFA-4 | **P2 性能/窗口** | 持锁区间过长 | `dealloc_frame` 的 `recycled.contains` O(n) 在持借+关中断内执行，扩大临界区 | 开放 |
| SFA-5 | **P2 数据竞争（多核）** | 多核未适配 | `UniprocessorSafeCell` + 无原子锁；多 hart 并发访问将数据竞争 | Baseline 不判错 |
| SFA-6 | **P1 API 绕过** | 守卫未覆盖 | `frame_allocator_cell()` 仍允许无 InterruptGuard 的 `exclusive_access()` | 开放（无当前调用方） |

### 2.6 当前支持范围

| 路径 | 状态 | 说明 |
|------|------|------|
| bring-up 自检 `test_with_range` | ✅ | 顺序初始化；堆先于帧池 |
| 单次 `frame_alloc_result` / `frame_dealloc_result` | ✅ | InterruptGuard + 短借；抢占安全 |
| `GlobalPhysFrameAllocator` 短借适配器 | ✅ | syscall lazy fault / COW 热路径 |
| 调度器临界区内帧分配 | ✅ | 中断守卫可嵌套 |
| 长持有 `frame_allocator_cell().exclusive_access()` | ❌ | API 仍导出；嵌套帧分配会 panic |
| 运行期 `init_frame_allocator` 重置 | ❌ | 无保护，可损坏池 |
| 多核并发 | ❌ | 未实现 |

### 2.7 收敛建议（剩余项）

**SFA-3** — `init_frame_allocator` 在 `FRAME_ALLOCATOR_READY` 已为 true 且池非空时：

```rust
log::warn!(
    "[frame-allocator] init_frame_allocator: reset while pool may be in use \
     range=[{:#x},{:#x}) stats={:?}",
    start_ppn.val, end_ppn.val, frame_mem_stats()
);
// 可选：非 test 配置下 return Err 或 panic
```

**SFA-4** — 用 `HashSet` / 位图替代 `recycled.contains`，或仅在 debug 校验重复回收。

**SFA-6** — 考虑将 `frame_allocator_cell()` 改为 `pub(crate)` 或文档标注「仅 bring-up，须自带 InterruptGuard」。

---

## 3. InterruptSafeLockedHeap / LockedHeap

### 3.1 数据结构与锁类型

```rust
struct InterruptSafeLockedHeap {
    inner: LockedHeap,  // linked_list_allocator：内部 spin::Mutex<Heap>
}

#[global_allocator]
static HEAP_ALLOCATOR: InterruptSafeLockedHeap;
```

- **内层锁**：`linked_list_allocator::LockedHeap` 的 `spin::Mutex`（自旋，**不可重入**）
- **外层守卫**：`with_allocator_interrupt_guard` — 先读/关全局中断，操作后按快照恢复

```rust
fn with_allocator_interrupt_guard<R>(f: impl FnOnce() -> R) -> R {
    let state = arch::interrupt::read_global_interrupt_state().ok();
    let _ = arch::interrupt::disable_global_interrupt();
    let ret = f();
    if let Some(state) = state {
        let _ = arch::interrupt::restore_global_interrupt_state(state);
    }
    ret
}
```

### 3.2 等价 lock / unlock 调用点

| 函数 | 文件:行 | 持锁区间 |
|------|---------|---------|
| `InterruptSafeLockedHeap::init` | `lib.rs:23-27` | 关中断 → `inner.lock()` → `Heap::init` → unlock → 恢复中断 |
| `GlobalAlloc::alloc` | `:33` | 关中断 → `LockedHeap::alloc`（内层 lock）→ unlock → 恢复中断 |
| `GlobalAlloc::dealloc` | `:37` | 同上 |
| `GlobalAlloc::realloc` | `:45-47` | 同上 |
| `heap_allocator::init` | `:85-86` | 调用 `HEAP_ALLOCATOR.init` |

**间接调用**：所有 `alloc` crate 路径（`Box`、`Vec`、`String`、驱动 `Vec`、页表 `Box::leak` 等）经 `#[global_allocator]` 进入。

### 3.3 持锁区间与闭环分析

**释锁闭环**：`spin::Mutex` guard + `with_allocator_interrupt_guard` 均为 RAII；各路径无手动漏释。

**关中断语义**：

- 与调度器 / 帧分配器 `InterruptGuard` 可嵌套：内层读到的 `SIE` 可能已为 0，恢复后仍保持外层预期的关中断状态。
- **缺陷**：`read_global_interrupt_state().ok()` 失败时（`state = None`），仍会 `disable_global_interrupt()`，且**不会恢复** → 首次堆操作后中断可能永久关闭（ISH-2）。

**持锁期间睡眠/调度**：

- 关中断期间不会响应定时器抢占（单核下），堆操作与抢占 **不** 直接冲突。
- 持 `spin::Mutex` 期间若触发 **同一线程嵌套 `GlobalAlloc`** → 自旋死锁（ISH-1）。

**与帧分配器交互**：

- 堆分配不调用帧分配器；帧分配器 `Vec` 增长调用堆 → 帧侧已关中断，堆侧再嵌套关中断，**无**交叉锁死锁；仅拉长帧侧临界区（SFA-2）。

### 3.4 潜在问题

| ID | 严重程度 | 类型 | 描述 | 状态 |
|----|---------|------|------|------|
| ISH-1 | **P0 卡死** | 不可重入自旋锁 | `spin::Mutex` 非递归。若 `GlobalAlloc` 回调链中（持内层 mutex）再次 `alloc`/`dealloc`，将 **无限自旋**。当前路径未见嵌套分配，但 **无编译期防护** | 开放 |
| ISH-2 | **P1 卡死** | 中断状态泄漏 | `read_global_interrupt_state().ok()` 为 `None` 时仍关中断且不恢复，后续定时器/调度失效。正常 bring-up 下 read 应成功；错误被 `.ok()` 吞掉 | 开放 |
| ISH-3 | **P2 语义偏差** | 失败即 panic | `handle_alloc_error` → `panic!`；OOM 不可恢复，非锁问题但影响「卡死」表象 | 开放 |
| ISH-4 | **P2 多核** | 自旋锁无 UP 优化 | `spin::Mutex` 在多核下需配合正确的中断屏蔽/核心亲和 | Baseline 不判错 |

### 3.5 当前支持范围

| 路径 | 状态 | 说明 |
|------|------|------|
| bring-up `heap_allocator::init` + `Vec` 测试 | ✅ | 引导阶段无并发 |
| 普通 `alloc` / `dealloc` / `realloc` | ✅ | 关中断 + 短临界区；无嵌套分配 |
| 调度器 / 帧分配器临界区内 `alloc` | ✅ | 中断守卫嵌套安全 |
| 中断处理器内 `alloc`（若未走 GlobalAlloc） | ❌ | 未覆盖；须禁止或专用池 |
| 持锁嵌套 `GlobalAlloc` | ❌ | 会死锁 |
| `read_global_interrupt_state` 失败 | ❌ | 中断永久关闭风险 |

### 3.6 收敛建议

**ISH-1** — 检测重入并 fail-fast（避免 silent spin）：

```rust
static HEAP_ALLOC_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
// compare_exchange 失败 → log::warn! + panic!("nested heap allocation")
```

**ISH-2** — 与帧分配器对齐，勿吞掉 read 失败：

```rust
let state = arch::interrupt::read_global_interrupt_state()
    .expect("heap-allocator: read_global_interrupt_state failed");
```

---

## 4. 两者交叉风险矩阵

| 场景 | 帧分配器状态 | 堆分配器状态 | 结果 |
|------|-------------|-------------|------|
| `frame_alloc` 正常路径 | RefMut + 关中断 | 未持锁 | ✅ 抢占安全（SFA-1 已修复） |
| `dealloc_frame` + `Vec::push` | RefMut + 关中断 | 短暂 mutex + 嵌套关中断 | ✅ 无交叉锁；临界区略长（SFA-2） |
| `heap alloc` 正常 | 未持借 | mutex + 关中断 | ✅ 安全 |
| `heap alloc` 嵌套 | 未持借 | 二次 mutex | ❌ ISH-1 自旋死锁 |
| 调度器 `InterruptGuard` + 帧分配 | 调度 RefCell + 帧 RefCell 嵌套关中断 | — | ✅ 无交叉 RefCell |
| 调度器 `with_scheduler` + `alloc` | — | 堆 mutex | ✅ 无交叉；若 alloc 触达调度器则调度 RefCell 死锁（属调度器审计） |

---

## 5. 调用链示意图

### 5.1 帧分配（syscall mmap lazy fault）— 修复后

```
用户 syscall
  → trap_handler (ecall)
  → dispatch_syscall_from_trap → sys_mmap
  → GlobalPhysFrameAllocator::alloc_frame
  → frame_alloc_result
  → with_frame_allocator
      → FrameAllocatorInterruptGuard::new  [关中断]
      → UniprocessorSafeCell::exclusive_access  [借出 RefMut]
      → StackFrameAllocator::alloc_frame
      → RefMut drop [释借]
      → FrameAllocatorInterruptGuard drop [恢复中断]
```

### 5.2 堆分配

```
Box::new / Vec::push
  → HEAP_ALLOCATOR::alloc (GlobalAlloc)
  → with_allocator_interrupt_guard
      → disable_global_interrupt
      → LockedHeap::alloc → spin::Mutex::lock
      → linked_list_allocator::Heap::allocate
      → spin::Mutex::unlock
      → restore_global_interrupt_state
```

---

## 6. 审计结论摘要

| 结构 | 持锁闭环 | 单核抢占 baseline | 主要缺口 |
|------|---------|------------------|---------|
| `StackFrameAllocator` | ✅ RefMut RAII + InterruptGuard | ✅ 关中断对齐调度器 | 运行期重复 init（SFA-3）；API 绕过（SFA-6） |
| `InterruptSafeLockedHeap` | ✅ Mutex + RAII | ✅ 关中断 | 不可重入（ISH-1）；read 失败路径（ISH-2） |

---

## 7. P0 / P1 / Fixed 摘要

### Fixed（本轮已修复）

| ID | 问题 | 修复 |
|----|------|------|
| **SFA-1** | 帧分配 RefCell vs 定时器抢占 → panic | `FrameAllocatorInterruptGuard` + `with_frame_allocator` 包裹全部导出 API；`expect` 处理中断读写失败 |

### P0（开放，易导致卡死）

| ID | 结构 | 问题 | 建议动作 |
|----|------|------|---------|
| **ISH-1** | `InterruptSafeLockedHeap` | 堆 `spin::Mutex` 嵌套 `GlobalAlloc` 自旋死锁 | 重入检测 + panic/warn；审计所有 alloc 钩子 |

### P1（开放，语义/可靠性）

| ID | 结构 | 问题 | 建议动作 |
|----|------|------|---------|
| **ISH-2** | `InterruptSafeLockedHeap` | 中断状态读取失败被 `.ok()` 忽略 | 改为 `expect`，与帧分配器守卫对齐 |
| **SFA-2** | `StackFrameAllocator` | 持借+关中断期间 `Vec` 堆分配拉长临界区 | 缩短持借；避免持借堆分配 |
| **SFA-3** | `StackFrameAllocator` | 运行期 `init_frame_allocator` 重置池 | warn + 非 test 拒绝 |
| **SFA-6** | `StackFrameAllocator` | `frame_allocator_cell()` 可绕过 InterruptGuard | 收紧可见性或文档约束 |

### P2（标注，baseline 不阻塞）

| ID | 问题 |
|----|------|
| SFA-4 | `dealloc_frame` O(n) `contains` |
| SFA-5 / ISH-4 | 多核未适配 |
| ISH-3 | OOM panic |

---

## 8. 文档回填

- 汇总至 `docs/audits/lock-issues.md`：SFA-1 **已修复**；ISH-1、ISH-2、SFA-3 仍开放
- 汇总至 `docs/audits/lock-coverage.md`：帧分配器运行期 syscall 路径应更新为 ✅
- 可选回填 `docs/exports/features/wateros-mm.md` / `wateros-runtime.md` 指向本文

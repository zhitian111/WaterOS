# fork/exit「越跑越慢」根因分析与修复方案

> 现象（用户实测）：**大量执行 fork 和 exit 之后，这两个步骤本身的速度会骤降**。
> 这是典型的「随累计操作数单调累积」退化，而非单次操作慢。本文按代码定位累积源，给出修复方案，并标注如何用运行时指标证实。

---

## 结论速览（按影响排序）

| 编号 | 根因 | 退化量级 | 为什么导致「fork+exit 同时骤降」 | 风险 |
|---|---|---|---|---|
| **D1** | 内核全局堆 = `linked_list_allocator`（first-fit 空闲链表） | **每次 alloc/dealloc O(空闲块数)，随碎片化单调上升，阈值式骤降** | fork/exit 都要分配/释放大量不同大小的堆对象；空闲链表越用越长越碎 → 两者一起变慢 | 中 |
| **D2** | 调度就绪队列 `OtherReadyQueue.versions` BTreeMap 永不回收 | O(累计 fork 数) 内存泄漏 + 每次调度 O(log N) | 每 fork 新增一条版本记录、永不删除；既拖慢调度，又持续撑大堆 → 加剧 D1 | 低 |
| **D3** | `alloc_pid`/`alloc_tid` 每次 fork 做 O(进程数×线程数) 扫描 | 仅当僵尸进程/线程累积时退化 | 若 reap 不及时，活进程数上升 → 每次 fork 的 PID 体检变慢 | 低 |

**D1 是「骤降」的主因**，D2 既是独立泄漏又会加速 D1，D3 是条件性放大器。建议三者一起处理。

---

## D1. 内核堆是 first-fit 空闲链表分配器（主因）

### 证据

```110:112:os/components/wateros-runtime/runtime-heap-allocator/src/lib.rs
#[global_allocator]
#[cfg(feature = "impl-linked-list-allocator")]
static HEAP_ALLOCATOR : InterruptSafeLockedHeap = InterruptSafeLockedHeap::empty();
```

内层是 `linked_list_allocator::LockedHeap`（`src/lib.rs:13,24`）。该分配器：

- **`alloc`**：遍历空闲「空洞」链表做 **first-fit**，复杂度 O(空洞数)。
- **`dealloc`**：按地址有序插入空洞链表并尝试合并相邻空洞，复杂度 O(空洞数)。

文件头注释也写明这是从 `buddy_system_allocator` 换来的：

```5:7:os/components/wateros-runtime/runtime-heap-allocator/src/lib.rs
//! **注意**：与之前的 buddy_system_allocator 不同，linked_list_allocator 使用非侵入式空闲链表，
//! 不会被堆内存本身的 use-after-free 破坏空闲链表元数据。
```

> 即：当初为了**健壮性**（避免 UAF 破坏分配器元数据）从 O(log n) 的 buddy 换成了非侵入式链表，但**牺牲了分配性能的上界**——把分配复杂度从 O(log n) 退化成 O(n 空洞)。

### 为什么 fork/exit 大量执行后骤降

- 一次 **fork** 在堆上分配的对象很多且大小各异：`Box<TaskControlBlock>`（registry.rs:231）、`ProcessControlBlock` 进 BTreeMap、fd 表/cwd/signal 复制、`versions`/wait 队列节点、页表中间结构等。
- 一次 **exit/reap** 释放这些对象。
- 这些对象**大小不一、释放顺序非 LIFO**，first-fit 链表会被切成越来越多的小空洞（外部碎片）。空洞链表长度随累计 fork/exit **单调增长**。
- 于是**每一次 alloc 的 first-fit 扫描 + 每一次 dealloc 的有序插入/合并都越来越慢**。当碎片度越过某个阈值后表现为「骤降」。
- 因为 fork 和 exit 都重度依赖堆，所以**两者会一起变慢**——与你的观测完全一致。

### 修复方案（按推荐度）

1. **改用有上界的分配器**（推荐）：
   - **TLSF**（如 `rlsf` crate）：alloc/dealloc **O(1)**，抗碎片，最契合「延迟稳定」诉求。
   - 或重新启用 **`buddy_system_allocator`**：O(log n)，按 2 的幂分级；当年换走是怕 UAF，但 UAF 应当作为正确性 bug 单独修，而不是用 O(n) 分配器来「容错」。
   - 或 **slab / 分级 free-list（segregated fit）**：对内核里高频固定大小对象（TCB、PCB、页表节点、固定大小 VecDeque 缓冲）按 size-class 命中，O(1) 且几乎无外部碎片。
2. **降低堆 churn**（与 1 互补）：
   - 为高频对象（TCB/PCB）做对象池复用，fork 取、exit 还，避免反复向全局堆要/还。
   - 修 D2 的泄漏，减少长期占用与碎片源。

### 如何证实（运行时指标）

- 该 crate 已暴露 `heap_mem_stats()`（`src/lib.rs:115`，返回 `used/free/capacity`）。在 fork/exit 压测循环里周期性打印：
  - 若 `used` 基本平稳但 fork/exit 时延随时间上升 → **碎片化**（空洞数增长）坐实 D1。
  - 若 `used` 持续单调上升 → 存在泄漏（很可能是 D2，叠加放大 D1）。
- 可临时在 `LockedHeap` 外包一层计数器统计空闲块个数，直接观察链表长度随时间增长。

---

## D2. 就绪队列版本表 `versions` 永不回收（独立泄漏 + 放大 D1）

### 证据（multi-class 与 round-robin 两份实现相同）

```36:43:os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/queues.rs
    fn bump_version(&mut self, task_id : TaskId) -> u64 {
        let entry = self.versions
                        .entry(task_id)
                        .or_insert(0);
        *entry = entry.saturating_add(1);
        *entry
    }
```

- `versions : BTreeMap<TaskId, u64>` 只在 `init()` 时 `clear()`；`enqueue_ready_task` / `detach_*` 都只 `bump_version`（`or_insert`），**没有任何路径删除条目**。
- 每次 fork 产生**全新 TaskId**（`registry.rs:63-71` 槽位复用但 `generation` 递增 → id 不同）。因此 `versions` **每 fork 净增一条、永不回收** → 大小 = 累计创建任务数。
- 影响：
  1. 每次 `enqueue_ready_task`/`pick_next` 的 `entry_is_live`/`bump_version` 变成 O(log N_total)；
  2. 持续向堆申请 BTreeMap 节点且不归还 → **`used` 单调上升、喂大 D1 的碎片**。

### 修复方案

- 在任务被 **reap/discard** 时同步删除其 `versions` 条目：给 `OtherReadyQueue` 加 `fn forget_task(&mut self, task_id)`（`self.versions.remove(&task_id)`），在 `MultiClassScheduler::reap_exited_task`/`discard_unstarted_task`（scheduler.rs:544-556）以及 round-robin 对应路径调用。
- 或者用「惰性收缩」：`pick_next` 弹出 stale 条目时若该 task 已不存在则 `versions.remove`。
- 配合：`ready_queue` 的 stale 条目当前靠 `pop_front` 漂白，正常 churn 下有界，无需大改；重点是 `versions` 的删除。

### 证实

- 打印 `versions.len()` 随 fork 次数变化：应当**有界**（≈并发就绪任务数）；若线性增长即坐实。

---

## D3. `alloc_pid` / `alloc_tid` 每 fork O(进程数 × 线程数) 扫描（条件性放大器）

### 证据

```112:125:os/components/wateros-task/task-impl/impl-core/src/process.rs
    fn alloc_pid(&mut self) -> ProcessId {
        loop {
            let pid = ProcessId::from_raw(self.next_pid);
            self.next_pid = self.next_pid.saturating_add(1);
            if !self.processes.contains_key(&pid) &&
               self.task_id_for_thread(ThreadId::from_raw(pid.raw())).is_none()
            {
                return pid;
            }
        }
    }
```

`task_id_for_thread`（`process.rs:364-373`）遍历**所有进程的所有线程**。`alloc_pid` 每次 fork 都调用它做一次「PID 不撞 TID」体检 → 单次 fork O(P×T)。

- `next_pid` 单调递增，正常情况下循环只跑一轮；但内层 `task_id_for_thread` 的 O(P×T) 与**活进程/线程数**成正比。
- 若 exit 后 reap 不及时（僵尸累积），P 上升 → fork 的 PID 体检变慢，且 exit 侧 `iter_tasks`/`has_child`/`find_exited_child`（registry.rs:393-406，O(slots 高水位)）也变慢 → **两端一起退化**，与现象一致。
- 注意：`TaskTable.slots` 这个 `Vec` 只增不缩（`remove` 只把槽位还入 `free_slots`，`slots.len()` 停在并发高水位），所以一旦某刻并发任务数冲高，之后所有 `iter_tasks` 都按高水位计费。

### 修复方案

- `alloc_pid` 去掉对 `task_id_for_thread` 的全表扫描：PID/TID 命名空间用**独立的位图/空闲栈**分配，O(1) 取号，不再每次做 O(P×T) 体检。
- exit 路径确保**及时 reap**（释放 PCB/TCB 槽），把活进程数压到真实并发量；必要时核对 lmbench/busybox 是否存在未 wait 的僵尸。
- 评估 `TaskTable.slots` 是否需要在空闲时收缩，或把 `has_child`/`find_exited_child` 从「遍历 slots」改为「父→子索引」。

### 证实

- 打印 `processes.len()` 与 `slots.len()` 随时间变化：若在 fork/exit 稳态下仍上升 → 僵尸/槽位累积坐实 D3。

---

## 建议实施顺序

1. **D1**：换分配器（TLSF/rlsf 或重启 buddy + 单独修 UAF）。这是「骤降」的根。可用 feature 切换并用 `heap_mem_stats` + fork/exit 压测对比时延曲线。
2. **D2**：`versions` 在 reap/discard 时删除（小改、低风险，顺带缓解 D1 的碎片来源）。
3. **D3**：PID/TID O(1) 分配 + 确认 reap 及时；如有僵尸再查 wait/reap 链路。

三项都属「让 fork/exit 时延随时间保持平稳」，与 lmbench `Process fork+exit` / `fork+execve` 直接相关，也利好所有反复起进程的功能用例（busybox/ltp/shell）的稳定性。

---

## 验证脚手架建议

在一个最小用户程序里循环 `fork()+_exit()`/`wait()` 数万次，每 N 次通过内核日志打印：
- `heap_mem_stats()` 的 `used/free`；
- `OtherReadyQueue.versions.len()`、`ready_queue.len()`；
- `ProcessRegistry.processes.len()`、`TaskTable.slots.len()`。

观察哪条曲线随循环次数单调上升，即可逐一坐实/排除 D1/D2/D3，并量化修复前后的时延平稳度。

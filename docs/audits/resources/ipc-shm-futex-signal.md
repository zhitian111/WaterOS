# 资源生命周期审计：ipc-shm-futex-signal

> **分组 ID**：`ipc-shm-futex-signal`  
> **覆盖资源**：#22 Futex 等待队列、#23 Futex robust 侧表、#24 进程/线程信号状态、#25 SysV SHM 段、#26 SHM 附着记录  
> **生成时间**：2026-06-25  
> **Baseline**：单核多线程；对照 Linux 常见 IPC/信号语义  
> **交叉参考**：[`syscall-issues.md`](../syscall-issues.md)（P0-08/09/10/11）、[`resource-inventory.md`](../resource-inventory.md) §#22–26

---

## 1. 分组摘要

| 资源 # | 名称 | 主要结构体 | 账本结论 | 最高风险 |
|--------|------|-----------|----------|----------|
| 22 | Futex 等待队列 | `FutexHub` → `BTreeMap<FutexKey, WaitQueue>` | **部分稳定** | key 不一致永久睡眠；异常退出不回收 `WaitQueueId` |
| 23 | Futex robust 侧表 | `BTreeMap<TaskId, RobustState>` | **部分稳定** | 退出清理仅 private wake；链表截断 |
| 24 | 信号状态 | `SignalRegistry`（process/thread + `real_deadlines`） | **部分稳定** | `real_deadlines` 陈旧项泄漏；临时 WaitQueue 不释放 |
| 25 | SysV SHM 段 | `ShmRegistry.segments` + 物理页 | **部分稳定** | 无全局段数上限；`shmget` 后未 attach 永驻 |
| 26 | SHM 附着 | `ShmRegistry.attachments` + `nattch` | **部分稳定** | fork MM 失败不回滚 `nattch`/附着表 |

**跨资源钩子**（已接线）：

| 事件 | futex / robust | signal | shm |
|------|----------------|--------|-----|
| `exit` / `exit_group` | `robust_exit_cleanup` → `drop_robust_list` | `on_thread_exit` / `drop_thread_state` | `drop_task_attachments` |
| `execve` | `robust_exit_cleanup_siblings_for_exec` | `on_exec` + 杀线程 `drop_thread` | 当前/被杀线程 `drop_task_attachments` + 换 aspace |
| `fork` / `clone` | 子线程新 `TaskId`，robust 不继承 | `fork_process` / `register_thread` | `fork_task_attachments` |
| 信号终止 | `apply_signal_dispatch(Terminate)` → robust 清理 | `drop_process` | 经 `kill_task` → reap → `drop_task_attachments` |
| reap 兄弟线程 | `drop_robust_state`（无用户链表遍历） | `drop_thread_and_empty_process` | `drop_task_attachments` |

---

## 2. 资源 #22：Futex 等待队列

### 2.1 分配入口

| 路径 | 函数 | 文件 |
|------|------|------|
| 惰性建队 | `FutexHub::get_queue` → `BTreeMap::entry.or_insert_with(WaitQueue::new)` | `ipc-futex/futex-impl/impl-task/src/hub.rs` |
| `FUTEX_WAIT` / `WAIT_BITSET` | `futex_wait` → `hub.wait_while` | `syscall-impl/impl-kernel/src/sys/futex.rs` |
| `FUTEX_REQUEUE` / `CMP_REQUEUE` | `FutexHub::requeue`（同时为 `to_key` 建队） | `hub.rs` |
| `clear_child_tid` / `wake_user_addr` | 双 key `wake_all`（private + shared） | `futex.rs` |

**前置依赖**：`futex` feature 启用 `impl-task`；调度器 `allocate_wait_queue()` 分配底层 `WaitQueueId`。

### 2.2 回收入口

| 路径 | 行为 |
|------|------|
| 正常 wait 返回后 | `cleanup_empty_queue` → `WaitQueue::try_release_empty` → 空则删 `BTreeMap` 项 |
| `wake` / `wake_all` / `requeue` 后 | 同上，对涉及 key 调用 `cleanup_empty_queue` |
| 任务在 futex 上阻塞时 `exit` | 调度器 `detach_task_from_run_queues` 从 `VecDeque` 移除 waiter；**不**调用 `FutexHub::cleanup_empty_queue` |
| 任务 reap | 无 futex 专用钩子 |

### 2.3 生命周期状态机

```text
[无表项] --首次 wait/requeue--> [FutexKey → WaitQueue 已分配]
        --waiter 入队--> [队列非空，WaitQueueId 占用]
        --wake/超时/信号中断--> [waiter 出队]
        --队列为空且 try_release_empty--> [表项删除，WaitQueueId 回 free 池]
        --任务 exit 而队列变空（未走 cleanup）--> [表项残留，WaitQueueId 泄漏]  ← 风险态
```

**半初始化**：`get_queue` 在首次 touch 时即分配 `WaitQueue` 与 `WaitQueueId`，即使随后 `EAGAIN` 未睡眠也会留下空表项（下次 wait 同 key 复用，一般可接受）。

### 2.4 账本稳定性

- **成对性**：正常 wait/wake 路径成对；**exit 路径不成对**（调度器摘链 ≠ FutexHub 回收）。
- **引用计数**：`WaitQueue` 为 `Copy` 句柄，无 RAII `Drop`；回收完全依赖显式 `try_release_empty`。
- **风险**：每个曾出现 wait 且以 **exit/kill** 结束（而非正常从 `wait_while` 返回）的唯一 `FutexKey`，可能永久占用一条 `BTreeMap` 项 + 一个 `WaitQueueId`。
- **key 语义**：`FutexKey { uaddr, is_private }`；WAIT 与 WAKE 若 private 标志不一致，历史上导致 **永久睡眠**（syscall P0-08）；已在 `futex_wake` 对零唤醒尝试 alternate key，**bitset 过滤仍未实现**（P0-09）。

### 2.5 耗尽处理

| 维度 | 现状 |
|------|------|
| Futex 表项上限 | **无**；`BTreeMap` 随唯一 key 增长 |
| `WaitQueueId` 池 | 无硬上限；`wait_queues: Vec` 单调扩展，仅靠 `free_wait_queues` 复用 |
| 帧/堆 | futex 本身不分配物理页；队列元数据在堆/Vec |
| 失败返回 | `EAGAIN`/`ETIMEDOUT`/`EINTR`/`ENOSYS`（不支持 op/bitset）；**无 ENOMEM** |

### 2.6 跨资源耦合

- 依赖 `ipc-waitqueue` → `wateros-task` 调度 wait 队列。
- 信号中断：`TaskWaitResult::Interrupted` → `EINTR`；与 `ipc-signal` 的 `interrupt_task` 联动。
- robust 退出 wake 使用同一 `FutexHub::wake_all`。
- `clone` 子进程共享 futex 用户地址，队列键为全局 uaddr（单 AS 地址空间模型下与 Linux 进程私有语义一致）。

### 2.7 潜在问题

| ID | 严重度 | 问题 |
|----|--------|------|
| FUT-P0-01 | **P0** | WAIT/WAKE **bitset** 非 `!0` 已收敛为 `-ENOSYS`，但 glibc 部分路径仍可能混用 bitset → 互斥失效或永久阻塞（见 syscall P0-09） |
| FUT-P0-02 | **P0** | private/shared key 不一致仍可睡眠（仅 wake 侧有 alternate 尝试）；**等待侧不尝试双 key** → 部分测例仍可能永久睡眠 |
| FUT-P1-01 | P1 | 任务 **exit/kill 于 futex 睡眠** 时不调用 `cleanup_empty_queue` → `FutexTables.queues` 与 `WaitQueueId` **泄漏** |
| FUT-P1-02 | P1 | 无全局 futex 表/队列 **限额** 与 `warn`；恶意或泄漏测例可撑大 `BTreeMap` 与 `wait_queues` Vec |
| FUT-P2-01 | P2 | `impl-dummy` 下全部 futex 返回 `ENOSYS`（非默认生产路径） |

### 2.8 收敛建议

1. 在 `detach_task_from_run_queues` 或 `FutexHub` 增加 **exit 钩子**：对空队列调用 `try_release_empty` 并移除表项。
2. WAIT 路径在入睡前可对 alternate private/shared key 做与 wake 对称的尝试（或统一规范化 key）。
3. 表项/WaitQueueId 达阈值时 `warn!` + 拒绝新 `get_queue`（`-ENOMEM` 或 `-EAGAIN`）。

### 2.9 修复任务草案

| 标题 | 文件 | 验收标准 |
|------|------|----------|
| futex exit 回收空队列 | `hub.rs` + `task.rs`/`wait_queues.rs` | 阻塞于 futex 的线程 `exit` 后，对应 key 表项删除且 `WaitQueueId` 回到 free 池 |
| futex 表项软上限 | `hub.rs` | 超限时 `warn` + 返回错误；LTP 长跑 `wait_queues.len()` 不无限增长 |
| WAIT 侧 alternate key | `futex.rs` | pthread 混用 private/shared 的测例不再永久睡眠 |

---

## 3. 资源 #23：Futex robust 侧表

### 3.1 分配入口

| 路径 | 函数 | 文件 |
|------|------|------|
| `set_robust_list(2)` | `FutexHub::set_robust_list` → `robust.insert(task, RobustState)` | `hub.rs`, `robust.rs` |
| 校验 | `len == ROBUST_LIST_HEAD_SIZE (24)`；`head!=0` 时读用户 `RobustListHead` | `robust.rs` |

每 **TaskId** 至多一条侧表记录（覆盖写）。

### 3.2 回收入口

| 路径 | 函数 | 说明 |
|------|------|------|
| 线程正常/异常退出 | `robust_exit_cleanup` → 遍历用户链表 → `drop_robust_list` | `sys/robust.rs`；`sys/task.rs` exit 路径 |
| exec 杀兄弟 | `robust_exit_cleanup_siblings_for_exec` | `execve.rs` |
| reap（无用户遍历） | `drop_robust_state` | 仅删侧表，不 wake |
| 信号终止全进程 | `apply_signal_dispatch` 中对各 member `robust_exit_cleanup` | `signal.rs` |

### 3.3 生命周期状态机

```text
[未登记] --set_robust_list--> [侧表: head,len]
         --线程运行--> [用户态维护链表内容；内核只存 head]
         --exit--> [读链表 → OWNER_DIED + wake → drop_robust_list]
         --reap 无 head--> [drop_robust_list，跳过 wake]
```

**半初始化**：`set_robust_list` 成功即占槽；用户链表内容由用户态维护，内核不持有链表节点内存。

### 3.4 账本稳定性

- **成对性**：`set_robust_list` / `drop_robust_list` 在已知 exit/reap 路径成对；**reap 路径不 wake**（与完整 robust 语义有差距）。
- **遍历安全**：`ROBUST_LIST_LIMIT = 4096` 步封顶，防坏链死循环；超长链表 **清理不完整**。
- **wake key**：`robust_exit_cleanup` **固定 `is_private: true`**（syscall SIG-P1-03）；共享 robust futex 可能不被唤醒。
- **`list_op_pending`**：非零时跳过处理（注释为 BusyBox 常规为 0）。

### 3.5 耗尽处理

| 维度 | 现状 |
|------|------|
| 侧表容量 | 每存活线程最多 1 条；随 `TaskId` 数量线性 |
| 遍历 | 4096 步后停止，无错误码反馈 |
| 失败 | `EINVAL`/`EFAULT`/`ESRCH`；`get_robust_list` 已修正三参数 ABI（syscall P0-10 已收敛） |

### 3.6 跨资源耦合

- 退出顺序：`robust_exit_cleanup` 在 `drop_task_runtime_resources` **之前**（`task.rs`），先于 shm/fd 回收。
- exec：先 robust 清理兄弟，再终止线程，再 shm detach。
- fork：**不**复制 robust 登记；子线程需自行 `set_robust_list`。

### 3.7 潜在问题

| ID | 严重度 | 问题 |
|----|--------|------|
| ROB-P1-01 | P1 | robust wake **仅 private key** → 共享 robust 锁泄漏 waiters |
| ROB-P1-02 | P1 | 链表 **>4096** 项时静默截断 → 遗留 futex 未 OWNER_DIED |
| ROB-P1-03 | P1 | `drop_robust_state`（reap）**不遍历唤醒** → 依赖其它路径清理用户 futex |
| ROB-P2-01 | P2 | `list_op_pending` 未实现 |

### 3.8 修复任务草案

| 标题 | 文件 | 验收标准 |
|------|------|----------|
| robust wake 双 key | `robust.rs` | 与 `wake_user_addr` 一致尝试 private/shared |
| 超长链表 warn | `robust.rs` | 触及 `ROBUST_LIST_LIMIT` 时 `warn!` 含 task_id、步数 |

---

## 4. 资源 #24：进程/线程信号状态

### 4.1 分配入口

| 路径 | 函数 | 文件 |
|------|------|------|
| 惰性注册 | `ensure_current_signal_state` → `register_process` | `signal.rs` |
| 多线程补登记 | `ensure_process_signal_state` → `register_thread` | `signal.rs` |
| fork | `on_fork` → `fork_process` | `signal.rs`, `clone.rs` |
| clone 线程 | `on_clone_thread` → `register_thread` | `signal.rs` |
| ITIMER_REAL 定时 | `set_timer` → `real_deadlines.entry(deadline).push` | `ipc-signal/src/lib.rs` |

**主要类型**：`ProcessSignalState`（actions、pending、三计时器）、`ThreadSignalState`（mask、pending、suspend/poll 暂存）、`real_deadlines: BTreeMap<u128, Vec<RealDeadlineEntry>>`。

### 4.2 回收入口

| 路径 | 函数 |
|------|------|
| 线程退出（最后或非最后） | `on_thread_exit` → `drop_thread`；末线程 `drop_process` |
| reap 辅助 | `drop_thread_and_empty_process` |
| 信号杀全进程 | `drop_process(pid)`（`apply_signal_dispatch`） |
| exec 杀线程 | `on_exec` → `drop_thread`（被杀线程）+ `exec_process`（保留线程重置 handler） |

**未回收**：`drop_process` / `drop_thread` **不**清理 `real_deadlines` 中该 pid 的陈旧 deadline 条目；`set_timer` 替换定时器时 **不**移除旧 deadline 索引。

### 4.3 生命周期状态机

```text
进程: [未注册] --register_process--> [ProcessSignalState]
      --fork--> 子进程新 state（复制 actions/mask，不复制 timer/pending）
      --exec--> 重置 caught handlers，保留 ignore/pending/timer
      --drop_process--> 删除 process + 过滤 threads

线程: [未注册] --register_thread--> [ThreadSignalState]
      --sigsuspend/poll--> 临时 mask（suspend_restore / poll_restore）
      --信号投递--> take_deliverable 修改 mask
      --drop_thread--> 删除线程项
```

**半初始化**：`begin_sigsuspend` / `begin_poll_sigmask` 未配对 `end_*` 时（如 kill），`suspend_restore_mask` 可能残留直至 `drop_thread`。

### 4.4 账本稳定性

- **process/thread 表**：在 exit/reap/exec 路径基本成对；惰性注册可能导致「任务已不存在但从未 drop」的极短窗口，由 reap 兜底。
- **`real_deadlines`**：**不可靠**；陈旧 `(deadline, RealDeadlineEntry)` 永久留在 `BTreeMap`，`expire_realtime` 靠 generation 跳过，造成 **索引泄漏**。
- **临时 WaitQueue**（与 #22 交叉）：
  - `rt_sigsuspend`：`WaitQueue::new()` 每次 syscall 分配新 `WaitQueueId`，**无 `try_release_empty`**（`WaitQueue` 为 `Copy`，无 `Drop`）。
  - `rt_sigtimedwait`：循环内同样每次 `WaitQueue::new()`。
  - 高频 sigsuspend 测例 → **`wait_queues` Vec 单调增长**（P0 级资源耗尽风险）。

### 4.5 耗尽处理

| 维度 | 现状 |
|------|------|
| NSIG | 64；`SignalSet` 为 u64 |
| 进程/线程表 | 无显式上限 |
| `real_deadlines` | 无上限；替换/删除 timer 不收缩 |
| 失败 | `EINVAL`/`ESRCH`/`EFAULT`；无 `ENOMEM` |

### 4.6 跨资源耦合

- `timer_tick`：CPU 计时 + `expire_realtime` → `send_process` → 可能 `interrupt_task`（P0-11 部分修复）。
- `ppoll`/`pselect6`：`begin_poll_sigmask` / `end_poll_sigmask`（P0-12 已收敛）。
- 终止信号：驱动 `robust_exit_cleanup`、进程 `kill_task`、`drop_process`。
- fork：复制 disposition 与 mask，**不**复制 pending/timer（与 Linux 一致，见单测）。

### 4.7 潜在问题

| ID | 严重度 | 问题 |
|----|--------|------|
| SIG-P0-01 | **P0** | `rt_sigsuspend` / `rt_sigtimedwait` 每次分配 **临时 WaitQueue 永不释放** → `WaitQueueId`/Vec **泄漏**，长跑可耗尽 |
| SIG-P1-01 | P1 | `real_deadlines` 在 `set_timer` 替换、`drop_process` 时 **不清理** → BTreeMap 永久膨胀 |
| SIG-P1-02 | P1 | 线程在 `begin_sigsuspend`/`begin_poll_sigmask` 中被 kill → 恢复 mask 依赖 `drop_thread`，可能短暂错误 mask |
| SIG-P1-03 | P1 | `apply_signal_dispatch(Terminate)` 对当前任务 `exit_group` 且对他人 `kill_task`，与 `drop_process` 顺序需与 task reap 一致（当前基本可用，缺集成测试） |
| SIG-P2-01 | P2 | `rt_sigaction` 不读用户 restorer（syscall SIG-P1-05） |

### 4.8 修复任务草案

| 标题 | 文件 | 验收标准 |
|------|------|----------|
| sigsuspend 队列 RAII | `signal.rs`, `task.rs` | syscall 返回前 `try_release_empty`；1e4 次 sigsuspend 后 `wait_queues.len()` 有界 |
| real_deadlines 清理 | `ipc-signal/src/lib.rs` | `set_timer` 移除旧 deadline；`drop_process` 移除该 pid 全部索引 |
| 临时 mask 异常恢复 | `ipc-signal/src/lib.rs` | `drop_thread` 时若 `suspend_restore_mask`/`poll_restore_mask` 存在则 `warn` 并丢弃 |

---

## 5. 资源 #25：SysV SHM 段

### 5.1 分配入口

| 路径 | 函数 | 文件 |
|------|------|------|
| `shmget(2)` | `ShmRegistry::create_or_get` | `ipc-shm/src/lib.rs`, `sys/shm.rs` |
| 物理页 | `alloc_segment_pages` → `frame_alloc_result` + `zero_page` | `ipc-shm/src/lib.rs` |

**条件**：`0 < size <= MAX_SHM_SEGMENT_SIZE (4 MiB)`；命名 key 需 `IPC_CREAT` 才创建；`IPC_PRIVATE` 总是新建段。

### 5.2 回收入口

| 路径 | 行为 |
|------|------|
| `shmdt` 致 `nattch==0` 且已 `IPC_RMID` | `remove_segment` → `frame_dealloc` 每页 |
| `shmctl(IPC_RMID)` | `mark_removed`；若 `nattch==0` 立即 `remove_segment` |
| `cancel_attach_reservation` | MM 失败时 `nattch--`，可能触发 `remove_segment` |
| 任务退出 | `drop_task` → 逐个 `detach_attachment`，可能 `remove_segment` |

**无回收**：`shmget` 成功但从未 `shmat` 且未 `IPC_RMID` 的段 **永久持有物理页**。

### 5.3 生命周期状态机

```text
[无段] --shmget--> [ShmSegment: pages,nattch=0]
      --shmat begin_attach--> [nattch++]
      --finish_attach--> [attachments 记录]
      --shmdt/exit--> [nattch--, unmap]
      --IPC_RMID--> [marked_removed]
      --nattch==0 && marked--> [remove_segment: 释放帧、删表项]
```

**半初始化**：`begin_attach` 后若 MM 映射失败，`cancel_attach_reservation` 回滚 `nattch`（**已实现对账**）。

### 5.4 账本稳定性

- **物理页**：`alloc_segment_pages` 失败时回滚已分配页；`remove_segment` 逐页 `frame_dealloc`。
- **nattch**：与 attach/detach 基本成对；**fork 路径有漏洞**（见 #26）。
- **shmid**：`next_id` 循环复用，冲突时扫描；耗尽返回 `ShmError::NoMem`。
- **key_index**：`IPC_RMID` 或 `remove_segment` 时移除；与段生命周期一致。

### 5.5 耗尽处理

| 维度 | 现状 |
|------|------|
| 单段大小 | **4 MiB** 硬上限 |
| 段数量 / 总 SHM 内存 | **无上限**；仅受物理帧池约束 |
| `shmget` 帧不足 | `ShmError::NoMem` → `-ENOMEM` |
| `shmctl` | 仅 `IPC_RMID`；其余 `-ENOSYS`（syscall MM-P1-05） |

### 5.6 跨资源耦合

- **MM**：`shmat`/`shmdt`/`fork` 经 `replace_range_with_shared` / `unmap_shared_range` 与 `Sv39AddressSpace` 同步。
- **fork**：COW aspace + `fork_task_attachments` 复制附着元数据并映射相同 PPN。
- **exec**：detach 所有附着（不销毁未 RMID 的段本身，与 Linux 一致）。
- **exit**：先 detach（降 `nattch`），再 drop fd/cwd 等。

### 5.7 潜在问题

| ID | 严重度 | 问题 |
|----|--------|------|
| SHM-P1-01 | P1 | **无全局段数/总字节上限**；`shmget`+不 attach 可泄漏物理页直至帧池枯竭 |
| SHM-P1-02 | P1 | `create_or_get` 对已有 key **不校验 size 是否匹配**（返回旧 shmid）→ 与 Linux 偏差，可能误导用户态 |
| SHM-P2-01 | P2 | `shmctl` 仅 RMID；`SHM_STAT` 等未实现 |

### 5.8 修复任务草案

| 标题 | 文件 | 验收标准 |
|------|------|----------|
| SHM 全局限额 | `ipc-shm/src/lib.rs` | 段数或总页数超限时 `warn` + `-ENOMEM` |
| shmget size 校验 | `create_or_get` | 已存在 key 且 size 不同 → `-EINVAL` |
| 孤立段回收策略 | 文档/可选 sysctl | 明确「仅 IPC_RMID+nattch==0」回收；测例要求 shmdt/RMID |

---

## 6. 资源 #26：SHM 附着记录

### 6.1 分配入口

| 路径 | 函数 |
|------|------|
| `shmat` 成功路径 | `finish_attach` → `attachments[task_id].push(ShmAttachment)` |
| `fork` | `fork_task` 复制父 `attachments` 并 `nattch++` |

### 6.2 回收入口

| 路径 | 函数 |
|------|------|
| `shmdt` | `detach` → 从 `attachments` 移除 + `detach_attachment` |
| 任务退出 | `drop_task` → 批量 detach |
| exec | `drop_task_attachments`（unmap + registry detach） |

### 6.3 生命周期状态机

```text
[无附着] --shmat finish--> [per-task Vec<ShmAttachment>]
         --fork--> 父子各一条记录，共享 shmid/PPN，nattch+=2
         --shmdt/exit--> 移除记录，nattch--
```

### 6.4 账本稳定性

- **shmat 错误回滚**：`reserve_attach_va` / `replace_range_with_shared` 失败时 `cancel_attach_reservation` + unmap（**稳定**）。
- **fork 不完整**（**不可靠**）：

```213:216:os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/clone.rs
    if let Err(error) = super::shm::fork_task_attachments(parent_id, child_id, new_aspace_ptr) {
        log::warn!("[sys_clone] failed to inherit shm attachments: {:?}",
                   error);
    }
```

`fork_task` 在 registry 内 **先** 增加 `nattch` 并写入子任务 `attachments`，**后** 在 syscall 层映射 MM。若 `replace_range_with_shared` 中途失败：

- registry 中子任务已登记全部附着；
- 子 aspace 可能 **部分映射**；
- 仅 `warn`，**不回滚** `nattch` 与子 `attachments`；
- 子进程后续 `shmdt` 可能 double-unmap 或漏 unmap → **帧与 nattch 账本漂移**。

- **exit 时 aspace=0**：`drop_task_attachments` 仍 detach registry，但 **跳过 unmap**（依赖进程 aspace 稍后整体销毁）；若 aspace 指针获取失败，依赖 MM 层 reap——需与 task-slots 审计交叉验证。

### 6.5 耗尽处理

| 维度 | 现状 |
|------|------|
| 每任务附着数 | **无专用 cap**（受 VA 与 rlimit 间接约束） |
| `nattch` 溢出 | `checked_add` 失败 → `Invalid`；fork 中 `usize::MAX` 时 **静默 skip** 该附着 |

### 6.6 潜在问题

| ID | 严重度 | 问题 |
|----|--------|------|
| ATT-P0-01 | **P0** | **fork shm 继承 MM 失败不回滚** registry → `nattch`/附着表与页表不一致，可导致 **帧泄漏或重复 dealloc** |
| ATT-P1-01 | P1 | `fork_task` 在 `nattch==usize::MAX` 时 **静默跳过** 附着，无 `warn` |
| ATT-P1-02 | P1 | `exit_group` 对兄弟 `drop_task_attachments(sibling, user_aspace)` 使用 **当前线程 aspace**；线程组共享 aspace 时正确，未来 `CLONE_VM` 分离 aspace 时会出错 |

### 6.7 修复任务草案

| 标题 | 文件 | 验收标准 |
|------|------|----------|
| fork shm 事务化 | `ipc-shm/src/lib.rs`, `shm.rs`, `clone.rs` | MM 映射失败时回滚子 `attachments` 与 `nattch`；失败返回 `-ENOMEM` 而非仅 warn |
| fork nattch 饱和 | `fork_task` | `nattch` 将溢出时 `warn` + 跳过并返回错误给 clone |

---

## 7. 分组级修复优先级队列（草案）

| 优先级 | ID | 摘要 | 组件 |
|--------|-----|------|------|
| **P0** | SIG-P0-01 | sigsuspend/sigtimedwait 临时 WaitQueue 泄漏 | signal + task |
| **P0** | ATT-P0-01 | fork shm MM 失败不回滚 nattch/附着 | ipc-shm + clone |
| **P0** | FUT-P0-02 | futex WAIT/WAKE key 不一致永久睡眠（wake 已部分修复） | futex |
| P1 | FUT-P1-01 | futex exit 不 cleanup 空队列 | futex + task |
| P1 | SIG-P1-01 | real_deadlines 索引泄漏 | ipc-signal |
| P1 | SHM-P1-01 | 无全局 SHM 段限额 | ipc-shm |
| P1 | ROB-P1-01 | robust wake 仅 private | robust |
| P2 | SHM-P1-02 / SHM-P2-01 | shmget size 校验 / shmctl 扩展 | ipc-shm |

---

## 8. 与 syscall 审计交叉索引

| syscall 问题 ID | 本资源 ID | 说明 |
|-----------------|-----------|------|
| P0-08 / P0-09 | FUT-P0-01 / FUT-P0-02 | futex bitset 与 key |
| P0-10 | — | get_robust_list ABI 已修复 |
| P0-11 | SIG-P1-02 | send_process interrupt（部分） |
| SIG-P1-03 | ROB-P1-01 | robust private wake |
| MM-P1-05 | SHM-P2-01 | shmctl 仅 RMID |

---

## 9. 账本总结论

| 资源 | 结论 | 一句话 |
|------|------|--------|
| #22 Futex 队列 | 部分稳定 | 正常 wait/wake 可回收；exit 与无上限表项是主要风险 |
| #23 Robust 侧表 | 部分稳定 | 登记/删除成对；wake 语义与遍历上限不完整 |
| #24 信号状态 | 部分稳定 | 线程/进程表尚可；**real_deadlines** 与 **临时 WaitQueue** 不可靠 |
| #25 SHM 段 | 部分稳定 | 页帧 alloc/free 成对；缺全局限额与孤立段策略 |
| #26 SHM 附着 | 不可靠 | shmat 回滚正确；**fork 失败路径破坏 nattch 账本** |

**单核多线程备注**：`FutexHub` 与 `SignalRegistry`、`ShmRegistry` 均用 `spin::Mutex` 全局锁保护；与锁审计交叉时关注 syscall 持锁顺序（shmget 持锁调帧分配、futex wait 不持 FutexHub 锁睡眠——当前实现先在临界区内 `get_queue` 后释放锁再 block，符合预期）。

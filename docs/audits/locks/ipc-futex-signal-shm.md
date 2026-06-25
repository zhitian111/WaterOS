# 锁机制审计：FutexHub / SignalRegistry / ShmRegistry

> 审计日期：2026-06-25（复核更新）  
> 分组：`ipc-futex-signal-shm`（lock-inventory #14–#16）  
> Baseline：单核多线程；`spin::Mutex` 为自旋锁；调度器为 `UniprocessorSafeCell` + `InterruptGuard`

---

## 1. 概述

本组三个全局 registry 均通过 `spin::Mutex` 保护，是 IPC 子系统（futex / signal / shm）的核心带锁结构。它们均从 syscall 层或定时器/生命周期钩子被调用，并与调度器（`wait_current_while` / `interrupt_task`）及 MM（`mmap` / `map_page_to_ppn`）存在交叉。

| 结构 | 文件 | 锁类型 | 保护内容 |
|------|------|--------|----------|
| `FutexHub` / `FutexTables` | `ipc-futex/futex-impl/impl-task/src/hub.rs` | `spin::Mutex<FutexTables>` | futex 等待队列表 + per-task robust 状态 |
| `SignalRegistry` | `ipc-signal/src/lib.rs` | `spin::Mutex<SignalRegistry>` | 进程/线程 signal 状态、定时器、realtime deadline 索引 |
| `ShmRegistry` | `ipc-shm/src/lib.rs` | `spin::Mutex<ShmRegistry>` | 段元数据、key 索引、per-task attach 列表 |

---

## 2. FutexHub 审计

### 2.1 锁 API 与调用点

| 操作 | 加锁方式 | 文件:行 |
|------|----------|---------|
| `with_tables` | `self.inner.lock()` | `hub.rs:31` |
| `wait_while` | 两次 `with_tables`（取队列 + 清理空队列） | `hub.rs:58,79` |
| `wake` / `wake_all` / `requeue` | `with_tables` 全程 | `hub.rs:91–98,111–138` |
| `set/get/drop_robust_list` | `with_tables` | `hub.rs:145–167` |

Syscall 入口：`sys/futex.rs`（`futex_wait/wake/requeue`）、`sys/robust.rs`（robust list + exit cleanup）。

### 2.2 持锁区间分析

**wait 路径（正确分离）**

```
with_tables → get_queue → 释锁
  → wq.wait_current_while(condition)   // 不持 FutexHub 锁
  → with_tables → cleanup_empty_queue → 释锁
```

`futex_wait` 的 `condition` 闭包在用户内存上复查 `uaddr` 值（`futex.rs:106`），**不在 FutexHub 锁内**，符合 Linux futex 语义。

**wake / requeue 路径（持锁跨越调度器）**

`wake` / `wake_all` / `requeue` 在 `with_tables` 持锁期间调用 `wq.wake_one()` / `wake_all()` / `requeue_to()`，这些函数经 `InterruptGuard` 进入 `with_scheduler`（`impl-round-robin/src/lib.rs:390–413`）。

持锁区间：**FutexHub Mutex → 调度器 UniprocessorSafeCell**。单核下不会形成 AB-BA 死锁（wait 路径先释 FutexHub 锁再进调度器），但会导致 wake 路径长时间自旋占用 FutexHub 锁，阻塞并发的 `wait_while` 取队列。

**robust_exit_cleanup**（`robust.rs`）：多次短持锁调用 `get_robust_list` / `wake_all` / `drop_robust_list`，中间有用户态拷贝，**不持 FutexHub 锁**，正确。

### 2.3 wait 与调度器交互（已修复 F-1）

调度器 `wait_current_while` / `wait_current_timeout_while` 在 `schedule_wait` 之后、 `__switch` 之前调用 `guard.release_before_switch()`，切换返回后由 `finish_wait_after_switch` 重新关中断并读取等待结果（`impl-round-robin/src/lib.rs:96–104,269–278,298–313`；`impl-multi-class` 同步）。

```
wait_current_while:
  InterruptGuard::new()
  condition()                    // 短暂关中断
  schedule_wait(...)
  release_before_switch()        // 切换前恢复中断
  finish_wait_after_switch:
    __switch(...)
    InterruptGuard::new()        // 返回后短暂关中断取结果
    take_current_wait_result()
```

此前未 `release_before_switch` 会导致阻塞任务继承「全局中断关闭」，timer tick / futex 超时 / realtime 信号失效（假死）。**当前代码已修复**。

futex 超时 `Some(0)` 在 `futex.rs:101–102` 于进入 hub 前返回 `ETIMEDOUT`；hub 内 `Some(0)` 分支（`hub.rs:66–71`）为冗余兜底，无死锁风险。

### 2.4 潜在问题

| ID | 严重度 | 状态 | 描述 |
|----|--------|------|------|
| F-1 | 卡死 | **已修复** | wait 路径跨 `__switch` 现已 `release_before_switch`（§2.3） |
| F-2 | 卡死/延迟 | 开放 | `wake`/`requeue` 持 FutexHub 锁期间调用调度器唤醒，持锁时间随唤醒数量线性增长 |
| F-3 | 语义偏差 | 开放 | `FUTEX_WAKE_BITSET` bitset 过滤未实现（`futex.rs:73–83` warn + ENOSYS） |
| F-4 | 数据竞争（多核） | 开放 | 全局 `spin::Mutex` 迁移多核须升级 |

### 2.5 当前支持范围

- 已覆盖：`FUTEX_WAIT/WAKE/REQUEUE/CMP_REQUEUE`（部分 bitset）、robust list 设置/退出清理、private/shared key、带超时 wait（调度器 tick）。
- 未覆盖/弱覆盖：`FUTEX_WAKE_BITSET` 非 `!0` bitset、`list_op_pending` 非零 robust 路径跳过。

---

## 3. SignalRegistry 审计

### 3.1 锁 API 与调用点

全局锁：`static SIGNAL_REGISTRY: Mutex<SignalRegistry>`（`lib.rs:677`），唯一入口 `with_registry()`（`lib.rs:679–682`）。

主要 syscall / 内核调用方：

| 调用方 | 操作 | 持锁模式 |
|--------|------|----------|
| `sys/signal.rs` | register/fork/exec/exit、send、timer_tick、deliver、sigsuspend | 多次独立 `with_registry`，无嵌套 |
| `sys/task.rs` | sigaction/sigprocmask/rt_sigtimedwait/setitimer/getitimer | 循环内多次短持锁 |
| `sys/kill.rs` | send_process + 遍历 interrupt | 两次独立持锁 |
| `sys/accept.rs` / `recvfrom.rs` | has_deliverable 轮询 | 短持锁 |

Registry 内部：`account_cpu` / `expire_realtime` 在持 `&mut self` 时调用 `send_process`（同一把锁的 RefMut，非递归 `with_registry`），**无重入死锁**。

### 3.2 持锁区间与调度器交互

**信号递送链**

```
send_thread/send_process  [with_registry]
  → apply_signal_dispatch  [无 registry 锁]
      Pending  → interrupt_task  [InterruptGuard + scheduler]
      Terminate → kill_task + with_registry(drop_process)
```

`apply_signal_dispatch` 在释锁后操作调度器，**无 SignalRegistry ↔ Scheduler 嵌套锁**。

**sigsuspend / rt_sigtimedwait**

```rust
// signal.rs:392-397, task.rs:507-521
wait.wait_current_while(|| {
    ipc::signal::with_registry(|registry| { ... })  // 条件闭包内抢 registry 锁
});
```

持锁顺序：**InterruptGuard → SignalRegistry（条件检查，切换前）→ 释锁 → release_before_switch → __switch**。

单核下：另一路径持 registry 锁时，等待任务在 condition 处自旋；持锁方很快返回，**不会永久死锁**。timer_tick 在中断恢复后可正常推进（F-1 修复后）。

**timer_tick**（`signal.rs:159–187`）

```
with_registry(account_cpu)   // 可能 send_process SIGVTALRM/SIGPROF
with_registry(expire_realtime)  // 可能 send_process SIGALRM
apply_signal_dispatch × N       // interrupt_task / kill
```

三次独立持锁，顺序清晰；`expire_realtime` 不清理 `set_timer` 替换前的 stale `real_deadlines` 条目（靠 generation 过滤），无锁问题但有冗余扫描。

### 3.3 潜在问题

| ID | 严重度 | 状态 | 描述 |
|----|--------|------|------|
| S-1 | 卡死 | **已修复** | 与 F-1 同源；wait 路径现已 `release_before_switch` |
| S-2 | 语义偏差 | 开放 | `kill()` 额外遍历全进程成员 interrupt（`kill.rs`），与 `send_process` 仅 interrupt `target_task_id` 不一致 |
| S-3 | 语义偏差 | 开放 | `send_process` 若所有线程均 mask 且非 `waiting_for`，`target_task_id=None`，Pending 入 process.pending 但不 interrupt |
| S-4 | 数据竞争（多核） | 开放 | 同 F-4 |
| S-5 | 低 | 开放 | `real_deadlines` stale 条目未在 `set_timer` 替换时移除 |

### 3.4 当前支持范围

- 已覆盖：rt_sigaction/mask/pending/suspend/timedwait、tkill/tgkill/kill、itimer（real/virtual/prof）、fork/exec/exit 生命周期、SIGCHLD 通知、用户 handler 帧构建。
- 弱覆盖：terminate 类信号的多线程语义、实时信号队列深度。

---

## 4. ShmRegistry 审计

### 4.1 锁 API 与调用点

`static SHM_REGISTRY: Mutex<ShmRegistry>`（`lib.rs:85`），导出 `registry()` 返回 `&'static Mutex<ShmRegistry>`。

| Syscall / 钩子 | 持锁区间 | 文件 |
|----------------|----------|------|
| `sys_shmget` | 全程 | `shm.rs:17` |
| `sys_shmctl(IPC_RMID)` | 全程 | `shm.rs:28` |
| `sys_shmat` | **分段**：`begin_attach` → 释锁 → MM → `finish_attach` | `shm.rs:45–82` |
| `sys_shmdt` | detach → 释锁 → unmap | `shm.rs:90–94` |
| `drop_task_attachments` | drop_task → 释锁 → unmap | `shm.rs:99–105` |
| `fork_task_attachments` | fork_task → 释锁 → map child | `shm.rs:113–116` |

Registry API（`lib.rs`）：

| API | 作用 |
|-----|------|
| `begin_attach` | 锁内 `nattch++`，返回 `ShmSegmentInfo`（`lib.rs:157–167`） |
| `finish_attach` | 锁内登记 attachment 元数据，不再次 `nattch++`（`lib.rs:170–195`） |
| `cancel_attach_reservation` | MM 失败或 `finish_attach` 失败时回滚 `nattch`（`lib.rs:198–207`） |
| `attach` | `begin_attach` + `finish_attach` 便捷组合（`lib.rs:145–154`） |

### 4.2 attach/detach 与 MM 路径（已修复 M-1）

**shmat 实际顺序（当前）**

```
1. lock → begin_attach (nattch++, clone pages) → unlock
2. reserve_attach_va → mmap 匿名区          [无 Shm 锁]
3. replace_range_with_shared → unmap+map    [无 Shm 锁]
   失败 → unmap + cancel_attach_reservation
4. lock → finish_attach (登记 attachment) → unlock
   失败 → unmap + cancel_attach_reservation
```

`begin_attach` 在 MM 映射前占位 `nattch`，使并发 `IPC_RMID`/`shmdt` 无法在映射期间将 `nattch` 降至 0 并 `remove_segment` 释放物理页。**TOCTOU UAF（原 M-1）已修复**。

**shmdt**：先 registry detach（`nattch--`，可能 `remove_segment`），再 `unmap_shared_range`。顺序正确。

### 4.3 潜在问题

| ID | 严重度 | 状态 | 描述 |
|----|--------|------|------|
| M-1 | UAF/数据损坏 | **已修复** | `begin_attach` / `cancel_attach_reservation` 两阶段 attach（§4.2） |
| M-2 | 语义/泄漏 | 开放 | `fork_task_attachments`：registry 先 `fork_task`（`nattch++` + child attachment），释锁后 `replace_range_with_shared`；映射失败时 `clone.rs:213–216` 仅 warn，未 `detach` 回滚 |
| M-3 | 延迟 | 开放 | `create_or_get` 持锁期间 `alloc_segment_pages` 逐页分配（最多 4MB） |
| M-4 | 低 | 开放 | `begin_attach` / `segment_info` 在锁内 `pages.clone()`，大段拷贝 |

### 4.4 当前支持范围

- 已覆盖：shmget（IPC_PRIVATE/CREAT/EXCL）、shmat/shmdt（两阶段 attach）、IPC_RMID 标记删除、fork/exit/exec 附件清理。
- 未覆盖：shmctl 除 RMID 外命令、SHM_LOCK/SHM_UNLOCK、权限检查（mode/cred）；fork shm 映射失败回滚（M-2）。

---

## 5. 跨结构锁顺序

```
典型顺序（无环）:
  ShmRegistry  ─┐
  SignalRegistry─┼─ 互相独立，syscall 路径无嵌套
  FutexHub      ─┘

FutexHub → Scheduler     (wake/requeue，持 FutexHub 锁)
Scheduler → SignalRegistry  (sigsuspend/rt_sigtimedwait condition，切换前)
Scheduler → FutexHub(短)    (wait_while 取队列，已释锁后 sleep)
```

单核 baseline 下**无确认的死锁环**；剩余主要风险为 **F-2 wake 持锁过长** 与 **M-2 fork attach 不一致**。

---

## 6. 收敛建议（开放项）

### F-2：缩短 wake 持锁

锁内仅收集 waiter 或 `WaitQueueId`，释锁后再 `wake_one_in_wait_queue`；或对过大 `max_wake` warn 并截断。

### M-2：fork attach 失败回滚

`fork_task_attachments` 映射失败时调用 `registry.lock().detach(child, base)`，或拆为 `fork_task_prepare` / `fork_task_commit` 两阶段。

### S-2 / S-3：语义对齐（非锁死锁）

统一 kill 与 send 的 interrupt 范围；全 mask 时考虑 wakeup 策略（属 signal 语义，非锁闭环）。

---

## 7. 问题汇总（按严重度）

| 优先级 | ID | 状态 | 类别 | 简述 |
|--------|-----|------|------|------|
| ~~P0~~ | M-1 | **已修复** | UAF | shmat 锁外 MM 与并发 RMID/detach 竞态 → `begin_attach` |
| ~~P0~~ | F-1/S-1 | **已修复** | 卡死 | wait 跨切换未释放中断 → `release_before_switch` + `finish_wait_after_switch` |
| P1 | M-2 | 开放 | 语义/泄漏 | fork shm 映射失败未回滚 registry attach |
| P1 | F-2 | 开放 | 卡死/延迟 | FutexHub wake 持锁跨调度器唤醒 |
| P2 | S-2/S-3 | 开放 | 语义偏差 | kill vs send interrupt 不一致；全 mask 不 wakeup |
| P2 | M-3 | 开放 | 延迟 | shmget 持锁逐页分配 |
| P3 | S-5 | 开放 | 性能 | real_deadlines stale 条目 |
| P3 | F-3/F-4/S-4/M-4 | 开放 | 语义/多核/性能 | bitset、多核 Mutex、pages.clone |

---

## 8. P0 / P1 / Fixed 摘要

### Fixed（本轮复核确认）

| ID | 修复点 | 证据 |
|----|--------|------|
| **M-1** | `ShmRegistry::begin_attach` / `finish_attach` / `cancel_attach_reservation`；`sys_shmat` 映射前占位 `nattch` | `ipc-shm/src/lib.rs:157–207`；`sys/shm.rs:45–82` |
| **F-1** | `wait_current_while` 等在 `__switch` 前 `release_before_switch` | `impl-round-robin/src/lib.rs:96–104,277,293,312` |
| **S-1** | 与 F-1 同源（sigsuspend / rt_sigtimedwait 共用 wait 路径） | 同上 + `signal.rs:392–397` |

### P0（当前无开放项）

原 P0（M-1、F-1/S-1）均已落地修复；单核 baseline 下无未解决的 P0 锁闭环缺陷。

### P1（待处理）

| ID | 风险 | 建议 |
|----|------|------|
| **M-2** | child registry 有 attach 记录但地址空间未映射（或部分映射失败） | 映射失败 `detach` 回滚或两阶段 commit |
| **F-2** | 高并发 wake 阻塞 futex wait 取队列 | 释锁后唤醒或限制 `max_wake` |

### P2 / P3

S-2、S-3（signal 语义）、M-3（shmget 持锁分配）、S-5 / M-4 / F-3 / F-4 / S-4（性能与多核迁移）。

---

## 9. 调用链速查

### futex wait 完整链

```
sys_futex → futex_wait → FutexHub::wait_while
  → [lock] get_queue [unlock]
  → WaitQueue::wait_current_while
      → scheduler::wait_current_while
          → condition (read_user_u32, IRQ off 短暂)
          → schedule_wait
          → release_before_switch
          → __switch
          → finish_wait_after_switch (IRQ off 短暂取结果)
  → [lock] cleanup_empty_queue [unlock]
```

### signal 递送链

```
timer_tick / sys_kill / sys_tkill
  → with_registry(send_*)
  → apply_signal_dispatch
      → interrupt_task / kill_task / drop_process
trap return → deliver_pending_signal
  → with_registry(take_deliverable)
  → 用户栈帧构建（锁外）
```

### shm attach 链（当前）

```
sys_shmat
  → [lock] begin_attach (nattch++) [unlock]
  → reserve_attach_va (MmapOps::mmap)
  → replace_range_with_shared (map_page_to_ppn)
      失败 → cancel_attach_reservation + unmap
  → [lock] finish_attach [unlock]
      失败 → cancel_attach_reservation + unmap
```

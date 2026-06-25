# TaskScheduler 锁机制审计（RoundRobin + MultiClass）

> 审计时间：2026-06-25（复核当前代码）  
> Baseline：单核多线程（UP）；`UniprocessorSafeCell` = `RefCell` 运行时独占借用  
> 关联清单：`docs/audits/lock-inventory.md` #3 `RoundRobinScheduler`、#4 `MultiClassScheduler`

---

## 1. 基本信息

| 项 | RoundRobin (#3) | MultiClass (#4) |
|---|---|---|
| **全局实例** | `static mut SCHEDULER: MaybeUninit<UniprocessorSafeCell<RoundRobinScheduler>>` | 同构，`MultiClassScheduler` |
| **主要文件** | `scheduler-impl/impl-round-robin/src/lib.rs`（加锁入口）、`scheduler.rs`（逻辑） | `scheduler-impl/impl-multi-class/src/lib.rs`、`scheduler.rs` |
| **锁类型** | `UniprocessorSafeCell<T>`（`exclusive_access()` → `RefCell::try_borrow_mut`） | 同上 |
| **同步辅助** | crate 内 `InterruptGuard`（关/恢复全局中断 RAII）+ `release_before_switch` | 同构 |
| **统一入口** | `with_scheduler(f)` = `scheduler_cell().exclusive_access()` + 闭包 | 同上 |
| **内嵌子结构** | `TaskRegistry`、`WaitQueues`、`OtherReadyQueue` | 另增 `RtFifoRunQueue`、`RtRrRunQueue` |

**锁语义**：调度器全部可变状态由**单一** `UniprocessorSafeCell` 包裹；内嵌 `TaskRegistry` / `WaitQueues` / 就绪队列**无独立锁**，仅能通过 `&mut Scheduler` 在 `with_scheduler` 闭包内访问。

**原语定义**：`os/components/wateros-base/src/sync/uniprocessor.rs`

```24:32:os/components/wateros-base/src/sync/uniprocessor.rs
    pub fn exclusive_access(&self) -> RefMut<'_, T> {
        match self.inner.try_borrow_mut() {
            Ok(inner) => inner,
            Err(_) => panic!(
                "RefCell already borrowed: {}",
                core::any::type_name::<T>()
            ),
        }
    }
```

**聚合层**：`task-scheduler/src/lib.rs`、`wateros-task/src/lib.rs` 仅 inline 转发至 `active_impl::*`，不额外加锁。

---

## 2. lock / unlock（`exclusive_access`）调用点

项目无显式 `lock()`/`unlock()`；等价 API 为 `UniprocessorSafeCell::exclusive_access()`，经 `with_scheduler` 封装。**释锁** = `RefMut` drop（闭包结束）。

### 2.1 原语层（每 impl crate 各 2 处直接 `exclusive_access`）

| 位置 | 操作 | 关中断 | 说明 |
|------|------|--------|------|
| `impl-*/src/lib.rs` `with_scheduler` | **借入** | 调用方负责 | 所有运行时访问的唯一入口 |
| `impl-*/src/lib.rs` `init_scheduler` | **构造 + init** | **否** | `UniprocessorSafeCell::new(...)` 后 `with_scheduler(\|s\| s.init())` |

### 2.2 经 `with_scheduler` 的公开 API（两 impl 对称，各 41 处闭包调用）

| 分类 | 函数 | InterruptGuard | 持锁跨 `__switch` | 备注 |
|------|------|----------------|-------------------|------|
| 地址空间 | `current_task_*` ×4 | ✓ | 否 | 短临界区 |
| 策略 | `apply_sched_policy_change` | ✓ | 否 | RR 仅改 TCB；MC 含 detach/入队 |
| 初始化 | `init_scheduler` | ✗ | 否 | boot 单线程假设 |
| 创建 | `spawn_kernel_task`, `spawn_user_task_spec` | ✓ | 否 | |
| 进程 | `fork_current`, `clone_current_thread`, `execve_current` | ✓ | 否 | |
| wait queue | `allocate_wait_queue`, `try_release_wait_queue` | ✓ | 否 | |
| **首次切换** | `run_first_task` | ✗ | N/A | **无 InterruptGuard** |
| 调度 | `suspend_current_and_run_next`, `schedule_tick`, `block_current` | ✓ | **guard 跨 switch**¹ | 释 RefCell 后 switch |
| **等待** | `wait_current`, `wait_current_while`, `wait_current_timeout*`, `sleep_current_for_ticks` | 分段² | 否（RefCell） | `release_before_switch` + `finish_wait_after_switch` |
| 唤醒/信号 | `wake_task`, `interrupt_task`, `kill_task` | ✓ | 否 | |
| 回收 | `reap_*`, `has_child` | ✓ | 否 | |
| wait queue 操作 | `wake_one/all_in_wait_queue`, `requeue_wait_queue` | ✓ | 否 | |
| 退出 | `exit_current` | ✓ | N/A | `release_before_switch` 后 switch，不返回 |
| 查询 | `current_task_id`, `*_snapshot`, `current_tick`, `current_task_kernel_stack_top` | ✓ | 否 | |
| trap | `begin_current_trap_frame_access`, `restore_current_trap_frame` | ✓ | 否 | begin 返回 raw 指针后锁已释 |

¹ `InterruptGuard` 对象留在调用栈上随上下文冻结；RefCell 借用已在 switch 前释放。切到 idle 时依赖 idle 任务主动 `enable_global_interrupt`（`runtime.rs:66–69`）。

² 等待/睡眠：`schedule_wait`/`schedule` 阶段持 guard → `release_before_switch()` 恢复中断 → `__switch`（无 RefCell、无 guard）→ 唤醒后 `finish_wait_after_switch` 内新建 guard 再 `take_current_wait_result`。

### 2.3 内层结构（无独立 lock）

`RoundRobinScheduler` / `MultiClassScheduler` 内所有 `registry`/`wait`/就绪队列方法均在 `scheduler.rs` 的 `&mut self` 上调用，持锁区间 = 外层 `with_scheduler` 闭包生命周期。

---

## 3. 调用链与持锁区间分析

### 3.1 标准模式：InterruptGuard + with_scheduler

```
调用方
  └─ InterruptGuard::new()           // 关中断
       └─ with_scheduler(f)          // exclusive_access → f(&mut s) → drop RefMut
  └─ InterruptGuard::drop()          // 恢复中断
```

**持锁（RefCell）区间**：仅 `f` 执行期间；**不**延续到 `__switch` 之后。

### 3.2 普通上下文切换（yield / tick / block）

```
InterruptGuard::new()
with_scheduler(|s| s.schedule(...))   // 持 RefCell → 释 RefCell，返回 SwitchPair
__switch(current, next)                 // 无调度器锁；guard 仍在栈上（中断仍关）¹
InterruptGuard::drop()                  // 若 __switch 返回则恢复中断
```

适用于：`suspend_current_and_run_next`、`schedule_tick`、`block_current`。

### 3.3 等待/睡眠路径（已修复 RC-1：三段式，guard 不跨 switch）

```
InterruptGuard::new()                              // ① 关中断
with_scheduler(|s| s.schedule_wait / schedule)     // ② 持 RefCell → 释 RefCell
guard.release_before_switch()                    // ③ 恢复中断（forget guard）
finish_wait_after_switch(switch_pair):
    __switch (可选)                                // ④ 无 RefCell、无 guard、中断开
    InterruptGuard::new()                          // ⑤ 唤醒后重新关中断
    with_scheduler(|s| s.take_current_wait_result()) // ⑥ 短临界区 → 释 RefCell
// ⑤ 的 guard drop → 恢复中断
```

实现（两 impl 同构）：

```96:105:os/components/wateros-task/task-scheduler/scheduler-impl/impl-round-robin/src/lib.rs
/// `__switch` 返回后重新关中断，再取等待结果（避免 wait 路径长期关中断，见锁审计 RC-1）。
fn finish_wait_after_switch(switch_pair : Option<SwitchPair>) -> TaskWaitResult {
    if let Some((current_task_cx_ptr, next_task_cx_ptr)) = switch_pair {
        unsafe {
            __switch(current_task_cx_ptr, next_task_cx_ptr);
        }
    }
    let _guard = InterruptGuard::new();
    with_scheduler(|scheduler| scheduler.take_current_wait_result())
}
```

```260:266:os/components/wateros-task/task-scheduler/scheduler-impl/impl-round-robin/src/lib.rs
pub fn wait_current(wait_handle : TaskWaitHandle) -> TaskWaitResult {
    let guard = InterruptGuard::new();
    let switch_pair = with_scheduler(|scheduler| scheduler.schedule_wait(wait_handle, None));
    guard.release_before_switch();
    finish_wait_after_switch(switch_pair)
}
```

**结论**：无持 RefCell 睡眠/调度；等待期间中断可响应 tick；唤醒后仅短临界区关中断取结果。

### 3.4 exit_current（不返回栈帧）

```
InterruptGuard::new()
with_scheduler(|s| s.schedule(Exit))    // 持 RefCell → 释 RefCell
guard.release_before_switch()           // 恢复中断，避免下一任务继承「永久关中断」
__switch                                // 不回到本帧
```

### 3.5 引导路径（无 InterruptGuard）

| 路径 | RefCell | 中断 | 风险 |
|------|---------|------|------|
| `init_scheduler` | ✓ | 未显式关 | boot 单线程假设 |
| `run_first_task` | ✓ | 未显式关 | 若 boot 末已开 timer → 重入 panic |

### 3.6 trap 帧指针（锁外窗口）

`begin_current_trap_frame_access` 在 `with_scheduler` 内修改 TCB，返回 `*mut TaskTrapFrame` 后 **RefCell 已释放**。trap handler 在锁外使用该指针直至 `restore_current_trap_frame`。依赖 trap 路径关中断/单核互斥。

### 3.7 与 ProcessRegistry 交叉（无嵌套持锁）

`wateros-task/src/lib.rs` 中 `spawn_user_task`、`fork_current`、`exit_current` 等**顺序**调用 scheduler 与 `with_process_registry`，不同时持有两把 RefCell。锁顺序不固定，当前无 AB-BA 嵌套死锁。

### 3.8 tick 中断入口

`os/src/trap_handler.rs:261` → `task::schedule_tick()` → impl `schedule_tick()` 内部自建 `InterruptGuard` + `with_scheduler`。trap 处理本身在 S 态中断上下文，与任务栈上的 guard 不嵌套 RefCell。

---

## 4. 潜在问题（按严重程度）

### P0 — 卡死 / panic

#### SCH-P0-1：RefCell 重入 panic

**机制**：`exclusive_access()` 在已有 `RefMut` 时 panic（`RefCell already borrowed: RoundRobinScheduler` / `MultiClassScheduler`）。

**触发条件**：
- 外层已持 RefCell，嵌套调用 `with_scheduler`（闭包内回调、condition 闭包内再进 scheduler 等）。
- 未关中断路径与 timer `schedule_tick` 并发（`run_first_task`、`init_scheduler` 无 guard）。
- `finish_wait_after_switch` 中 `__switch` 返回后、新建 guard 前的极短窗口：timer 可运行 `schedule_tick`（UP 下由中断上下文持锁，不与前任务 RefCell 冲突；见 SCH-P1-2）。

**缓解**：运行时 API 普遍 `InterruptGuard` → `with_scheduler`；tick 路径自建 guard。

**收敛建议**（§6.1）：`with_scheduler` 入口增加重入检测 + warn；debug 维持 panic。

#### SCH-P0-2：`run_first_task` 无 InterruptGuard

**位置**：`impl-*/src/lib.rs:218–224`。

```218:224:os/components/wateros-task/task-scheduler/scheduler-impl/impl-round-robin/src/lib.rs
pub fn run_first_task() -> ! {
    let (current_task_cx_ptr, next_task_cx_ptr) =
        with_scheduler(|scheduler| scheduler.prepare_first_switch());
    unsafe {
        __switch(current_task_cx_ptr, next_task_cx_ptr);
    }
```

若引导末尾 timer 已使能，可与 `prepare_first_switch` 的 RefCell 借用并发 → panic 或状态损坏。生产路径通常中断仍关，风险取决于 boot 时序。

**收敛建议**（§6.3）：补 `InterruptGuard::new()`。

---

### P1 — 数据竞争 / 语义窗口 / 可解释卡顿

#### SCH-P1-1：yield/tick 路径 InterruptGuard 跨 `__switch`（idle 补偿）

**位置**：`suspend_current_and_run_next`、`schedule_tick`（非 wait 路径）。

guard 随被切换任务栈冻结，切到 idle 时 **SIE 仍为关**；依赖 `__wateros_idle_task_runtime_main` 主动 `enable_global_interrupt()`，否则 `wfi` 等不到定时器（`runtime.rs:66–69`）。wait/exit 已用 `release_before_switch` 规避；yield/tick **未**采用同样模式。

**影响**：非 idle 目标时，被切换任务唤醒后至 syscall 返回前中断仍关，可能延迟 tick；与历史「卡死」现象部分相关。

**收敛建议**：评估 yield/tick 是否对齐 wait 的 `release_before_switch` + 短临界区模式；或文档化 idle 补偿为硬性契约。

#### SCH-P1-2：`finish_wait_after_switch` 唤醒后极短无 guard 窗口

`__switch` 返回后至 `InterruptGuard::new()` 前中断开启。UP 下 timer 可插入 `schedule_tick`，不导致 RefCell 重入（不同执行上下文），但可能在 `take_current_wait_result` 前触发额外调度。当前实现依赖唤醒后该任务尽快取结果；极端抢占下行为需回归测试。

#### SCH-P1-3：trap 帧 raw 指针锁外使用

**位置**：`begin_current_trap_frame_access` → trap handler 使用 TCB 内指针直至 `restore_current_trap_frame`。

trap 路径关中断下 UP 可控；trap 内再入 scheduler 或多核下失效。

#### SCH-P1-4：`sched.rs` / 聚合层多次独立加锁 TOCTOU

例：`ensure_task_exists` → `task_snapshot`（释锁）→ `apply_sched_policy_change`（再加锁）。两次间任务可被 kill/reap。语义竞态，非死锁。

#### SCH-P1-5：`spawn_user_task` 等与 ProcessRegistry 非原子

scheduler 创建任务后释锁，再更新 ProcessRegistry；中间窗口 task 存在但 process 未登记。见 `process-registry` 审计。

#### SCH-P1-6：`block_current`  latent 风险

当前代码库**无调用方**，但若未来用于「阻塞后 __switch 返回同一栈帧」路径，仍持 guard 跨 switch（同旧 wait 语义），未使用 `release_before_switch`。新增调用前应比照 wait 路径改造。

---

### P2 — 语义偏差（非锁死锁）

#### SCH-P2-1：RoundRobin `apply_sched_policy_change` 不迁移就绪队列

**位置**：`impl-round-robin/src/scheduler.rs:43–55` — 仅 `set_task_sched`，无 detach/入队。MultiClass 完整迁移。策略变更后队列与 TCB 可能不一致；非数据竞争，但 RT 测试在 RR feature 下可能「优先级无效」。

#### SCH-P2-2：`kill_task` 无法杀当前任务

`WaitQueues::kill_task` 对 `current_task_id == task_id` 返回 false；`exit_group_current` 依赖此语义。非锁 bug。

---

### P3 — 多核备注（baseline 不视为错误）

- `UniprocessorSafeCell` 仅适用于单核；多 hart 须换 spin mutex 等。
- `unsafe impl Sync` 依赖调用约束，非硬件保证。

---

## 5. 当前支持范围（coverage）

### 5.1 已正确覆盖

| 路径 | RefCell 加锁 | 关中断 | 释锁闭环 | `__switch` 前释 RefCell |
|------|-------------|--------|----------|-------------------------|
| 运行时 syscall 调度/等待/唤醒 | ✓ | ✓ | ✓ | ✓ |
| tick → `schedule_tick` | ✓ | ✓ | ✓ | ✓ |
| wait/sleep（RC-1 修复后） | ✓ | 分段 | ✓ | ✓ |
| `exit_current` + 中断移交 | ✓ | `release_before_switch` | ✓ | ✓ |
| trap 帧 begin/restore | ✓ | ✓ | ✓ | N/A |
| 任务创建 / fork / clone / execve | ✓ | ✓ | ✓ | N/A |
| kill / wake / interrupt / reap | ✓ | ✓ | ✓ | N/A |

### 5.2 部分覆盖 / 依赖假设

| 路径 | 状态 |
|------|------|
| `init_scheduler` | boot 单线程，无 InterruptGuard |
| `run_first_task` | 无 InterruptGuard |
| yield/tick switch | guard 跨 switch，idle 主动开中断 |
| trap 帧 raw 指针 | 依赖 trap 互斥 |
| RR 策略变更 | 不迁移队列 |
| `block_current` | API 存在，无调用方 |

### 5.3 未覆盖

内嵌 `TaskRegistry` / `WaitQueues` / 就绪队列无独立 lock API；若从 scheduler 外直接引用将绕过保护。

---

## 6. 收敛建议

### 6.1 SCH-P0-1：`with_scheduler` 重入检测

**位置**：`impl-round-robin/src/lib.rs`、`impl-multi-class/src/lib.rs` 的 `with_scheduler`。

```rust
log::warn!(
    "[scheduler-lock] {} op=exclusive_access_reentrant loc={}:{}",
    "RoundRobinScheduler", // 或 MultiClassScheduler
    file!(),
    line!()
);
// 返回 Err(SchedError::Busy) 或 debug panic
```

### 6.2 SCH-P0-2 / 6.3：`run_first_task` 补 InterruptGuard

```rust
pub fn run_first_task() -> ! {
    let _guard = InterruptGuard::new();
    let (cur, next) = with_scheduler(|s| s.prepare_first_switch());
    ...
}
```

### 6.3 SCH-P1-1：yield/tick 对齐 wait 的 guard 分段（可选）

对 `suspend_current_and_run_next` / `schedule_tick` 评估 `release_before_switch` + switch 后短 guard，减少对 idle 补偿的依赖；需验证与 trap/tick 嵌套语义。

### 6.4 SCH-P1-3：trap 帧 borrow 标志（debug）

debug 下 per-task「trap 帧 borrow 中」标志；持标志时再次 `with_scheduler` 修改同一 TCB → warn。

### 6.5 SCH-P2-1：RoundRobin 策略变更

对非 `SchedPolicy::Other` warn + `SchedError::NotSupported`，或移植 MultiClass 的 detach/enqueue 逻辑。

---

## 7. 锁顺序小结

| 锁 A | 锁 B | 当前代码 | 风险 |
|------|------|----------|------|
| Scheduler RefCell | 自身 | 禁止嵌套；InterruptGuard 防 tick 重入 | 重入 → panic |
| Scheduler | ProcessRegistry | 顺序调用，不嵌套 | 低 |
| Scheduler | VFS/futex/其他 | syscall 顺序调用 | 需全局汇总 |

**推荐顺序**（新代码）：关中断 → 持 Scheduler RefCell → 释 RefCell → 持其他 registry → 恢复中断。避免持 RefCell 调用可能阻塞的下层。

---

## 8. 已修复项（当前代码确认）

| ID | 问题 | 修复位置 | 说明 |
|----|------|----------|------|
| **RC-1** | wait/sleep 路径 InterruptGuard 长期跨 `__switch`，唤醒后仍关中断 | `finish_wait_after_switch` + `release_before_switch`（`impl-*/src/lib.rs:96–105, 260–345`） | 两 impl 同构；注释引用锁审计 RC-1 |
| **RC-2** | `exit_current` 下一任务继承关中断 | `guard.release_before_switch()` before `__switch`（`lib.rs:417–431`） | 与 idle enable 形成互补 |
| **RC-3** | `__switch` 前持 RefCell | 所有 switch 路径先结束 `with_scheduler` 闭包再 `__switch` | 无持锁睡眠 |
| **RC-4** | `UniprocessorSafeCell` panic 信息 | `uniprocessor.rs:26–31` `try_borrow_mut` + type_name | 便于定位重入类型 |

---

## 9. 审计结论

TaskScheduler（RoundRobin / MultiClass）采用**单 RefCell 包裹全状态** + **InterruptGuard 防中断重入**；两实现**锁协议同构**，差异在调度语义（RR 单队列 vs MC 多类队列）。

**核心正确**：RefCell 在 `__switch` 前释放；wait/sleep/exit 已通过 `release_before_switch` / `finish_wait_after_switch` 修复长期关中断（RC-1/RC-2）。

**剩余高优先级**：RefCell 重入 panic（SCH-P0-1）、`run_first_task` 无 guard（SCH-P0-2）、yield/tick guard 跨 switch 依赖 idle 补偿（SCH-P1-1）。

---

## 附录：清单 #3 / #4 映射

| 清单 # | 结构 | 本文档章节 |
|--------|------|-----------|
| 3 | `RoundRobinScheduler` | §1–§5、§8；§4 SCH-P2-1 RR 特有问题 |
| 4 | `MultiClassScheduler` | §1–§5、§8；策略变更见 MC `apply_sched_policy_change` |

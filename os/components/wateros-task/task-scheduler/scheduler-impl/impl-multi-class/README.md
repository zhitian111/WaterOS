# Multi-class Scheduler 实现手册

[调度器总览](../../README.md) · [Scheduler API](../../scheduler-api/api-v0/README.md) · [Task Core](../../../task-impl/impl-core/README.md)

本实现以一把全局 `MultiprocessorSafeCell<MultiClassScheduler>` 锁保护 TaskRegistry、WaitQueues 和全部 CPUState。runqueue 按 CPU 分开，但状态迁移仍在同一锁内完成；IPI 发送和真实 `__switch` 在锁外进行。

## 源码地图

| 文件 | 职责 |
| --- | --- |
| `src/lib.rs` | 全局初始化/锁、当前任务原子快照、锁外 IPI、公开 schedule/wait/wake 入口。 |
| `src/scheduler.rs` | `MultiClassScheduler`、首次/普通切换和目标上下文校验。 |
| `scheduler/runqueue.rs` | 选核、入队/出队、pick 和可选任务迁移。 |
| `scheduler/policy.rs` | policy/priority/nice/ioprio/affinity 修改与抢占。 |
| `scheduler/tasks.rs` | 创建、fork/clone、exec、kill、reap。 |
| `scheduler/wait.rs` | wait queue、timeout、wake/requeue。 |
| `scheduler/cpu.rs`、`query.rs` | online/timekeeper/负载/诊断。 |

## 调度顺序与切换边界

选择顺序为 FIFO/RR 实时类优先，然后 OTHER/BATCH 公平类，再用户 SCHED_IDLE，最后物理 idle TCB。物理 idle 是带真实内核栈的任务，并非 `None`。

```text
timer/yield/block/exit/reschedule
  -> 保存中断状态并关中断
  -> with_scheduler：同步 current cache，迁移旧状态，pick next
  -> set_current_task：MM aspace leave(old)/enter(new)
  -> 构造 SwitchPair，校验 next RA/SP
  -> 取出 pending_reschedule_cpus
  -> 释放 scheduler 锁
  -> 向远端发送 IPI
  -> __switch(current, next)
  -> 切换完成后发布 deferred ready 任务
```

绝不能在上下文尚未保存时把运行中任务放入其它 CPU 队列，否则两个 CPU 可同时使用同一内核栈。`CURRENT_TASK_IDS`、`CURRENT_ASPACE_PTRS`、`CURRENT_TICK` 是供锁内 condition/诊断避免递归加锁的原子镜像，不允许用它们直接修改 scheduler 状态。

`set_current_task` 在地址空间变化时通知 MM leave/enter，用于 TLB shootdown 目标集合。任何绕过它更新 CPU current 的新路径都会造成地址空间销毁或 TLB 跟踪错误。

## 锁、中断和 IPI

公开入口用 RAII 保存并关闭本 CPU 中断后加 scheduler 锁，退出恢复原状态。锁内累计 `pending_reschedule_cpus`，锁外发送定向 IPI，避免 IPI handler 反向等待同一锁。远端 IPI 只设置/消费重调度请求，不推进全局 tick。

目标上下文的返回地址若落入 kernel heap，`validate_switch_target` 会 panic，因为这通常表示 TCB/栈已释放或上下文被覆盖。遇到该错误应追查任务发布/回收时序和内核栈所有权，不要扩大允许地址范围。

## 修改状态迁移的检查表

1. 在一个 scheduler 临界区内摘除旧容器、更新 TCB、设置 CPU cache、加入新容器。
2. 确认 current/ready/running CPU 三个字段同步，异常路径不会重复入队。
3. 需要远端抢占时只记录目标，锁外发 IPI。
4. 若切换地址空间，经过统一 `set_current_task`。
5. 若回收 TCB，先从全部 run/wait/timeout 索引移除，并确保不再 current/deferred。
6. 压力验证 SMP=1 与 SMP=8、频繁 fork/exit、wait timeout、affinity 改动和远端 wake。

运行期应联合观察 CPU snapshot 的 runnable 分项、current、need_resched、timer/context-switch 计数，以及 debug 的 scheduler lock wait。长期 ready 但从不运行通常检查 affinity/online/queue ownership；上下文损坏则检查 premature reap/deferred publish；超时速度异常则检查 timekeeper 唯一性。


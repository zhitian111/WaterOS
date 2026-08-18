# Scheduler API v0 与基础队列手册

[调度器总览](../../README.md) · [Task API](../../../task-api/api-v0/README.md)

本 crate 不只是 trait 声明，还提供 `TaskRegistry`、`CPUState`、FIFO/RR/CFS 队列和 `WaitQueues` 的基础实现。multi-class 层负责把这些容器组成原子调度事务。

## 主要结构

- `TaskRegistry`：`TaskId -> TaskControlBlock`，负责创建、状态标记、trap frame 和 TCB 回收。
- `CPUState`：每 CPU current/idle、online、need_resched、五类 runqueue、统计和当前快照 cache。
- `CfsQueue`：按 vruntime 排序，同 vruntime 用 FIFO；用于 OTHER/BATCH/用户 SCHED_IDLE。
- `FifoQueue`/`RrQueue`：实时 priority 1..99 分桶；RR 另有当前时间片计数。
- `WaitQueues`：显式等待队列、task/child exit 等待、sleep、timeout、blocked 和 exited 容器。
- `ScheduleReason`/`QueueTarget`：把 tick/yield/block/sleep/exit 转为当前任务下一状态。

## 队列归属不变量

一个普通任务只能在以下位置之一：某 CPU ready queue、某 CPU current、一个 wait/sleep 容器、exited queue，或“已创建但尚未发布”。从一种容器转移到另一种时必须在全局 scheduler 锁内完成摘除、TCB 状态更新和重新入队。

`CPUState.current_snapshot` 是 TCB 的运行期 cache：tick 可就地推进 vruntime/统计，切出前必须同步回 registry。它不能成为第二份永久真相。被强制迁移的当前任务先记入 `deferred_ready_after_switch`，要等 `__switch` 已保存旧上下文后再发布到其它 CPU。

## 等待与超时

```text
条件等待者（持外部对象锁）
  -> scheduler 锁内再次检查 condition
  -> 条件未满足：TCB -> Blocking，登记 target 和可选 timeout
  -> 释放外部锁并切换
唤醒者
  -> 外部状态已更新
  -> wake/requeue
  -> 从所有 wait/timeout 索引摘除
  -> finish_wait(Woken/TimedOut/Interrupted)
  -> 选择合法 CPU 入 ready queue
```

复查条件与入等待队列必须组成无丢唤醒协议。`try_release_wait_queue` 仅在无 waiter 时成功，并清除名字/timeout；ID 可复用，因此外部对象销毁后不能继续保存旧 ID。只有 timekeeper CPU 推进 `current_tick`，否则 SMP 数量会改变超时速度。

futex requeue 等需要同时维护 scheduler wait queue 与外部 waiter 元数据；只返回 changed 数不足以同步两边，因此实现层使用真实 `woken`/`moved` TaskId 列表。

## 修改调度算法

新增 policy 或队列时至少同步：`SchedPolicy` 与参数校验、CPUState 字段/init/load、enqueue/dequeue/pick、抢占规则、tick、yield、affinity 迁移、快照/诊断以及 syscall ABI。检查队列计数在重复 dequeue 和状态异常时不会下溢。

定向测试应覆盖同优先级 FIFO、RR 时间片、CFS 相同 vruntime、yield 不立即选回自己、实时抢占公平类、空队列回 idle、CPU offline/affinity 无交集、timeout 与 wake 同 tick 竞态。

## 错误边界

API容器不应静默修复重复enqueue、错误queue owner、计数下溢或非法状态迁移；debug构建应尽早失败并输出TaskId/CPU/target，release路径也必须拒绝制造第二份归属。调用者在同一scheduler事务中处理错误，不能留下半更新TCB。

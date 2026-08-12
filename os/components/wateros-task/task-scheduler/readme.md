# wateros-task-scheduler

[项目首页](../../../../README.md) · [内核工程](../../../README.md) · [wateros-task](../readme.md)

`wateros-task-scheduler` 是 WaterOS 的多类、每 CPU 调度器。当前实现支持五种 Linux 调度
策略：`SCHED_OTHER`、`SCHED_BATCH`、`SCHED_IDLE`、`SCHED_FIFO`、`SCHED_RR`；并为每个配置
CPU 维护 idle task、current task、runnable 队列、timer/switch 统计与重调度状态。

## 分层

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 聚合 | `src/lib.rs` | 选择 `impl-multi-class` 并直接导出。 |
| API | `scheduler-api/api-v0/` | task registry、CPU state、五类 runnable 队列、wait queue 数据结构。 |
| 实现 | `scheduler-impl/impl-multi-class/` | 调度决策、上下文切换、SMP 放置、生命周期与 wait 操作。 |
| 外层 | `wateros-task` | syscall/trap/IPC 可调用的门面与 process 协调。 |

实现文件按领域拆分：

| 文件 | 内容 |
| --- | --- |
| `scheduler.rs` | `MultiClassScheduler`、首次切换、普通调度、tick 与地址空间切换通知。 |
| `scheduler/cpu.rs` | CPU online、timekeeper、snapshot、CPU 负载与重调度请求。 |
| `scheduler/tasks.rs` | spawn、fork、clone、exec、task 查询与 affinity。 |
| `scheduler/lifecycle.rs` | yield/block/sleep/exit、ready placement、运行队列状态转换。 |
| `scheduler/policy.rs` | 调度策略、优先级和 nice 相关入口。 |
| `scheduler/wait.rs` | wait queue 分配、等待、唤醒和 requeue。 |

## 关键数据结构

```text
MultiClassScheduler（唯一 scheduler 锁保护）
 ├─ TaskRegistry       : TCB、TaskState、上下文、父子关系、ready/last/running CPU
 ├─ WaitQueues         : WaitQueueId -> waiter、sleep deadline、woken/timeout 集合
 ├─ CPUState[MAX_CPUS] : online/current/idle、Other/Batch/Idle/FIFO/RR 队列、need_resched、统计
 ├─ timekeeper_cpu     : 唯一推进全局 timeout 的 CPU
 └─ pending_reschedule_cpus : 锁内积累、锁外发送的定向 IPI 目标
```

`CPUState` 中的 idle task 是实际可切换的内核任务，不是 `None`。`current_task_id ==
idle_task_id` 表示 CPU 正在 idle；只有 CPU 尚未完成 `run_first_task` 或 offline snapshot
才可能没有 current task。

## 调度流程

```text
timer tick / yield / block / sleep / exit / remote IPI
  -> scheduler 锁内更新当前任务状态与队列归属
  -> 选当前 CPU 上最高类别的下一个 runnable task，空时选择 idle
  -> 更新 current_task、`Running` + `running_cpu_id`、地址空间 active CPU 状态
  -> 释放 scheduler lock 与 interrupt guard
  -> __switch(current_context, next_context)
```

选择顺序与队列语义如下：

1. `SCHED_FIFO` 与 `SCHED_RR` 是实时类，按 priority `99 → 1` 扫描；同一 priority 下 FIFO
   先于 RR。
2. `SCHED_OTHER` 和 `SCHED_BATCH` 共用 fair class，跨队列选择最小 vruntime；相等时 Other
   优先，Batch 因此轻微劣后但不会被饿死。
3. `SCHED_IDLE` 使用独立的 vruntime 基线，仅在前述 runnable 队列为空后运行。
4. 所有用户/内核 runnable 队列为空时，运行每 CPU 专属的物理 idle task。

RR 受时间片驱动轮转；FIFO 不因本地时间片强制轮转。无论策略如何，任务被选中后都必须先从
ready queue 取出，再标记 `Running` 并记录 `running_cpu_id`；运行中任务不能同时保留在 ready
queue。

## 状态与队列原子性

下列动作必须在**同一把 scheduler 锁**内完成：

1. 从旧 ready/wait/sleep 容器移除任务；
2. 更新 `TaskState`；
3. 更新 `ready_cpu`、`last_cpu`、`running_cpu` 与 CPU 的 `current_task_id`；
4. 将任务加入唯一的新容器，或标记为 `Running`（连同 `running_cpu_id`）/`Exited`；
5. 若远端 CPU 需要观察新工作，写入 `pending_reschedule_cpus`。

实际 IPI 发送、日志、MM 回调和 `__switch` 都在锁外。禁止持 scheduler 锁等待、调用 VFS/IPC
回调或进入用户内存路径。

## Wait queue 与 timeout

`WaitQueues` 是 scheduler 的一部分，而不是独立 IPC scheduler。`WaitQueueId` 可被
`wateros-task::WaitQueue` 和 `ipc-waitqueue` 引用。

- 条件等待由 `wait_current_while` 在 scheduler 临界区复查条件；条件已改变时不会阻塞。
- `wake_one` / `wake_all` 只激活仍处于匹配 `Blocking(WaitQueue(id))` 状态的任务，陈旧项会被
  丢弃，避免重复 Running。
- requeue 同时修改 waiter 容器和任务的 `Blocking` target。
- `try_release_wait_queue` 仅能在没有 waiter 且无上层并发句柄时调用；futex 等动态队列还需要
  自己的引用使用计数，避免 ID 复用。

全局 tick 由 `timekeeper_cpu` 单独推进 timeout。其他 CPU 的 timer tick 不推进 sleep/wait
deadline，只更新本 CPU 的时间片、统计和抢占判断。

## SMP 放置与 IPI

- CPU online 后有自己的 idle/current state 与五类 runnable 队列。
- `LeastLoaded` 为新任务选择 runnable 最少的 online 且符合 affinity 的 CPU，平局按轮转起点
  打散；`LastCpu` 用于唤醒时尽量保持 cache locality。
- 远端 ready queue 入队会设置目标 CPU `need_resched` 和 pending CPU mask；锁外只向目标
  online CPU 发 IPI。
- 软中断 IPI 只消费控制原因并触发重调度判断，不能当作 timer tick 推进全局时间。

## 扩展与测试

新增调度策略或队列时，要同时修改 scheduler API 的 queue/registry、`CPUState`、调度选择、
任务入队/出队、snapshot 以及本文件。新增跨 CPU 行为还必须说明锁顺序、IPI 目标和 timeout
归属。

基础验证：

```sh
make -C os rv_check
make -C os la_check
```

运行时排障可调用 `cpu_states()` / `print_cpu_states()`，结合每 CPU 的 current、队列长度、
`need_resched`、switch/timer 计数判断任务是否错误地滞留在 ready、wait 或 idle 路径。

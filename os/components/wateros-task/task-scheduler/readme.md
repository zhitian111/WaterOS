# wateros-task-scheduler

[项目首页](../../../../README.md) · [内核工程](../../../README.md) · [wateros-task](../readme.md)

`wateros-task-scheduler` 是 WaterOS 的多类、每 CPU 调度器。它维护 TCB、就绪队列、等待队列
和每 CPU 调度状态，并负责把任务生命周期变化转换成安全的队列迁移、调度决策和上下文切换。

## 模块分层

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 聚合层 | `src/lib.rs` | 选择并导出当前的多类调度器实现。 |
| 调度器 API | `scheduler-api/api-v0/` | 定义并实现 TaskRegistry、CPUState、CFS/FIFO/RR 队列和 WaitQueues。 |
| 调度器实现 | `scheduler-impl/impl-multi-class/` | 实现调度决策、任务放置、生命周期转换、上下文切换和 SMP 重调度。 |
| 任务门面 | `wateros-task/src/` | 协调 scheduler 与 process，并向 syscall、trap 和 IPC 提供稳定接口。 |

多类调度器实现按职责拆分如下：

| 文件 | 内容 |
| --- | --- |
| `src/lib.rs` | 全局 scheduler 容器、锁外 IPI 派发和 yield/block/sleep/exit 等入口。 |
| `src/scheduler.rs` | `MultiClassScheduler`、首次运行、普通调度和上下文切换准备。 |
| `src/scheduler/cpu.rs` | CPU online、timekeeper、负载统计、快照和重调度请求。 |
| `src/scheduler/tasks.rs` | task 创建、发布、查询、exec 和 affinity。 |
| `src/scheduler/policy.rs` | policy、priority、nice 和抢占判断。 |
| `src/scheduler/runqueue.rs` | 就绪队列选择、入队、出队和 CPU 放置。 |
| `src/scheduler/wait.rs` | wait queue 分配、条件等待、唤醒、timeout 和 requeue。 |
| `src/scheduler/query.rs` | task/CPU 查询以及调度停滞诊断。 |

## 实现说明

- 调度器支持五种策略：`SCHED_OTHER`、`SCHED_BATCH`、`SCHED_IDLE`、`SCHED_FIFO` 和
  `SCHED_RR`。
- 每个配置 CPU 都拥有独立的 current task、物理 idle task、五类就绪队列、`need_resched`
  状态及 timer/context-switch 统计；当前不实现运行中任务迁移和 work stealing。
- `SCHED_OTHER`、`SCHED_BATCH` 和 `SCHED_IDLE` 使用 vruntime 公平队列。Other 与 Batch
  共享公平类选择次序，SchedIdle 仅在其它策略没有 runnable task 时参与选择。
- FIFO 和 RR 是实时策略，使用 1 到 99 的 priority 分桶。FIFO 不因普通时间片轮转，RR
  达到时间片后重新排队；更高实时优先级仍可抢占当前任务。
- 每 CPU 的物理 idle task 是实际存在且能够切换到的内核任务，不等同于 `None`，也不同于
  用户可设置的 `SCHED_IDLE` 策略。
- 全局 `MultiClassScheduler` 由一把多核安全锁保护。TaskState、队列归属、ready/running CPU
  和 CPU current 必须在这把锁内一起更新。
- 新任务优先放到符合 affinity 的最空 online CPU；唤醒任务优先回到 `last_cpu_id`，目标离线
  或过载时再重新选择。
- 只有 timekeeper CPU 推进全局 sleep/wait timeout；其它 CPU 的 timer 只处理本地 vruntime、
  时间片、统计和抢占，避免 CPU 数量改变超时速度。
- 远端任务入队后只通知实际目标 CPU。IPI 只触发重调度判断，不推进 scheduler tick。

## 调用链路

初始化流程：

```text
scheduler::init()
  -> 创建 MultiClassScheduler、TaskRegistry 和 WaitQueues
  -> 为每个配置 CPU 创建 CPUState 与物理 idle TCB
  -> 将启动 CPU 标记 online，并指定全局 timekeeper CPU
```

任务创建和发布分为两个阶段：

```text
create_*_task()
  -> 在 TaskRegistry 中创建尚未入队的 Ready TCB
  -> 上层完成 PCB、fd、credential、signal 等资源初始化
activate_ready_task()
  -> 按 affinity、online 状态和负载选择目标 CPU
  -> 更新 ready_cpu_id，并加入目标 CPU 的唯一就绪队列
  -> 设置 need_resched 和 pending_reschedule_cpus
  -> 释放 scheduler 锁后向远端目标发送定向 IPI
```

普通调度流程：

```text
tick / yield / block / sleep / exit / reschedule
  -> 锁内同步当前任务快照和运行统计
  -> 按调度原因更新当前任务状态及所属容器
  -> 从本 CPU 选择下一任务；无普通任务时选择物理 idle task
  -> 将下一任务出队，标记 Running，并更新 CPU current
  -> 准备地址空间和 TaskContext 切换
  -> 释放 scheduler 锁
  -> __switch(current_context, next_context)
  -> 旧任务以后恢复时释放中断 guard，并处理延迟发布的迁移任务
```

调度锁会在 `__switch` 前释放，但中断 guard 有意跨越实际上下文切换：这样可以避免调度器已将
另一任务登记为 current、CPU 却仍在旧任务栈上时响应调度中断。被切走的旧任务恢复后才释放
自己的 guard；第一次运行的任务由任务入口完成对应收尾。

等待与唤醒流程：

```text
wait_current_while()
  -> scheduler 临界区内再次检查条件
  -> 条件仍成立时将当前任务放入 WaitQueues，并切走

wake_one() / wake_all()
  -> 从 WaitQueues 取出仍匹配等待目标的任务
  -> 写入 wait result，并按 LastCpu/LeastLoaded 重新发布
  -> 锁外向实际远端 CPU 发送重调度 IPI
```

## TaskRegistry实现功能

`TaskRegistry` 的主要实现在 `scheduler-api/api-v0/src/registry.rs`，内部使用
`BTreeMap<TaskId, Box<TaskControlBlock>>` 保存所有调度实体。

- 为物理 idle、内核任务和用户任务分配 TaskId 并保存 TCB。
- 支持创建初始用户任务，以及 fork、clone 和 exec 所需的 TCB 操作。
- 维护 `Ready`、`Running`、`Blocking`、`Sleeping`、`Exited` 状态。
- 维护 `ready_cpu_id`、`running_cpu_id` 和 `last_cpu_id`，供队列归属检查、唤醒放置和诊断使用。
- 保存 policy、priority、nice、vruntime、运行统计、等待结果和 TaskContext。
- 提供稳定的 `TaskSnapshot`，避免 dashboard 和诊断代码直接长期借用 TCB。
- 任务退出后先保留 Exited TCB 供 wait/reap 观察，确认回收时再从 registry 移除。

新建但尚未发布的任务可以是 `Ready + ready_cpu_id=None`；一旦正式发布，Ready 任务必须恰好
位于一个 CPU 的一个就绪队列中。Running 任务必须已经出队，且其 `running_cpu_id` 与对应
CPU 的 current task 一致。

## CPUState与就绪队列实现功能

`CPUState` 及各队列主要位于 `scheduler-api/api-v0/src/cpu.rs`、`cfs_queue.rs`、
`fifo_queue.rs` 和 `rr_queue.rs`。

- `CPUState` 保存 online、current、物理 idle task、`need_resched`、调度统计和延迟发布任务。
- current task 使用独立快照缓存热路径数据；tick 更新缓存中的 vruntime 和统计，任务离开 CPU
  时再同步回 TaskRegistry。
- Other、Batch 和 SchedIdle 各有一条 `CfsQueue`。队列使用
  `BTreeMap<VRunTime, VecDeque<TaskId>>`，优先选择最小 vruntime，同一 vruntime 下保持 FIFO。
- nice 通过权重影响 vruntime 增长：权重越大，同样运行时间增加的 vruntime 越少，长期获得的
  CPU 份额越多。任务进入另一 CPU 的公平队列时会按目标队列基线归一化，避免低 vruntime
  任务长期插队。
- FIFO 和 RR 按 priority 分桶，并从 99 向 1 选择；相同 priority 下 FIFO 先于 RR。
- 当前选择顺序为：实时 FIFO/RR → Other/Batch → `SCHED_IDLE` → 物理 idle task。
- 新任务使用 LeastLoaded 放置，负载相同时通过轮转起点避免总是选择 CPU 0；唤醒任务使用
  LastCpu 放置以保留缓存局部性，并在目标不合适时回退到 LeastLoaded。
- affinity 修改不会让正在运行的任务立即出现在远端队列。源 CPU 先将它切走，待 `__switch`
  已保存旧上下文后再延迟发布，避免同一个 TaskContext 同时在两个 CPU 上运行。

## WaitQueues实现功能

`WaitQueues` 的主要实现在 `scheduler-api/api-v0/src/wait_queues.rs`，调度器侧操作位于
`scheduler-impl/impl-multi-class/src/scheduler/wait.rs`。

- 分配和释放 `WaitQueueId`，维护显式等待队列、task-exit、child-exit、blocking、sleeping 和
  exited 等等待容器。
- 支持无期限等待、带 deadline 等待、wake-one、wake-all 和跨队列 requeue。
- timeout 队列按 deadline 排序，只由 timekeeper CPU 推进并激活到期任务。
- 条件等待在 scheduler 锁内复查条件，保证“检查条件”和“登记 waiter”之间不会丢失唤醒。
- 唤醒时会再次核对任务状态和等待目标；已经退出、已被其它路径唤醒或不再匹配的陈旧 waiter
  会被丢弃，避免重复入队或重复 Running。
- requeue 会同时更新 waiter 所在容器和 TCB 的 Blocking target，保持两边语义一致。
- 动态 wait queue 的上层使用者还必须维护自己的引用生命周期，只有无 waiter、无并发使用者时
  才能释放 ID，避免编号复用后误唤醒其它对象。

## 调度器实现功能

`MultiClassScheduler` 是所有调度状态的协调者，主要实现在
`scheduler-impl/impl-multi-class/src/scheduler.rs`。

- 聚合 TaskRegistry、WaitQueues、每 CPU 状态、任务放置轮转点、timekeeper CPU 和待重调度
  CPU mask。
- 在同一临界区完成任务状态转换、旧容器移除、新容器加入、CPU current 更新和重调度请求，
  防止任务同时处于两个队列或两个 CPU。
- 根据 ScheduleReason 处理 tick、yield、block、sleep、wait、exit 和显式 reschedule；只有确实
  需要切换时才准备 `__switch`。
- 维护用户地址空间的 active CPU 状态，使 MM 在页表修改时能够确定 TLB shootdown 目标。
- 锁内只累计 `pending_reschedule_cpus`；释放 scheduler 锁后，本地目标执行本地重调度判断，
  远端目标通过平台 SMP 接口发送定向 IPI。
- IPI 到达后先消费目标 CPU 的 `need_resched`。请求已经被本地一次 schedule 消费时不会递归
  再调度；IPI 合并或延迟时，本地 timer 仍作为重调度兜底。
- `cpu_states()`、`task_snapshot()` 和 `log_stall_diagnostics()` 提供 CPU online、current、
  runqueue、等待目标、timer、switch 和 reschedule 状态，供 dashboard 与 GDB 诊断使用。

调用方通常应使用 `wateros-task` 的聚合接口，不应直接操作全局 scheduler。新增调度路径时必须
继续保持三条边界：状态与队列在 scheduler 锁内原子更新；IPI 在锁外发送；上下文真正保存完成
之前，旧任务不得发布到其它 CPU。

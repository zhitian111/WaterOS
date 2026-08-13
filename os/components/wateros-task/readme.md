# wateros-task

[项目首页](../../../README.md) · [内核工程](../../README.md) · [系统架构](../../../README.md#系统架构)

`wateros-task` 是 WaterOS 的进程、线程与调度器聚合模块。模块内部维护 process registry 和
scheduler 两组全局状态，分别负责进程语义管理和任务调度管理；顶层 `src/` 负责协调两者，
并向 syscall、IPC、trap 等模块提供统一接口。

## 模块分层

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 聚合门面 | `src/lib.rs` 及 `src/*.rs` | 聚合 process 和 scheduler 接口，协调任务创建、生命周期、等待与 SMP 状态。 |
| 任务 API | `task-api/api-v0/` | 定义 Task、Process、调度策略、等待目标与快照等公共语义类型。 |
| 任务实现 | `task-impl/impl-core/` | 实现 TCB、PCB、ProcessRegistry 及对应的多核安全访问接口。 |
| 调度器 API | `task-scheduler/scheduler-api/api-v0/` | 定义并实现 TaskRegistry、CPUState、CFS/FIFO/RR 队列和 WaitQueues。 |
| 调度器实现 | `task-scheduler/scheduler-impl/impl-multi-class/` | 实现调度决策、状态转换、上下文切换、SMP 任务放置与重调度。 |

## 实现说明

- 进程与线程采用一对多模型：一个 PCB 可以维护一个 leader task 和多个 member task。
- `TaskId` 是调度器内部可运行实体的编号；`ProcessId` 是用户态 PID；`ThreadId` 是用户态
  TID。用户任务同时存在于 scheduler 的 TaskRegistry 和 ProcessRegistry 中。
- 早期实现中一个 task 基本对应一个进程；加入线程后，进程级地址空间和进程关系由 PCB
  维护，每个 TCB 作为一条可独立调度的执行流，拥有自己的内核栈、上下文和 trap frame。
- 内核任务和每 CPU 的物理 idle task 只有 TCB，不一定具有 PCB；用户任务必须登记到 PCB。
- 调度器支持五种策略：`SCHED_OTHER`、`SCHED_FIFO`、`SCHED_RR`、`SCHED_BATCH`、
  `SCHED_IDLE`。其中 Other、Batch、Idle 使用基于 vruntime 的公平队列；FIFO 和 RR 使用按
  priority 分桶的队列。
- 支持多核调度。每个 CPU 维护自己的 current/idle task、五类就绪队列、timer/switch 统计和
  `need_resched`；全局 scheduler 维护 TaskRegistry、WaitQueues、timekeeper CPU 和待发送 IPI
  的 CPU mask。
- scheduler 和 process registry 分别由多核安全锁保护。实际 IPI、地址空间回调、信号投递和
  `__switch` 不应在持有这些锁时执行。
- credential、文件描述符、CWD、mount namespace 和 signal 等资源由其它模块的侧表维护。
  用户任务通常采用“创建但不入队 → 初始化全部侧表 → 发布到就绪队列”的两阶段流程，避免
  其它 CPU 提前运行尚未初始化完成的任务。

## 调用链路

对 TCB 的访问通过 scheduler 内的 TaskRegistry；对 PCB 的访问通过 ProcessRegistry。普通调用
方应使用 `wateros-task/src` 暴露的聚合接口，而不是直接跨过聚合层操作两个 registry。

初始化流程：

```text
task::init()
  -> scheduler::init()              初始化 TaskRegistry、CPUState 和每 CPU idle task
  -> init_process_registry()        初始化 ProcessRegistry
```

用户任务创建流程：

```text
create_user_task(user)
  -> scheduler 创建尚未入队的 TCB
  -> ProcessRegistry 创建 PCB，并建立 PID/TID/TaskId 索引
  -> 调用方初始化 credential、fd、cwd、signal 等侧表
start_user_task(task_id)
  -> 选择 online 且符合 affinity 的 CPU
  -> 标记 Ready 并加入目标 CPU 的唯一就绪队列
  -> 必要时向目标 CPU 发送定向重调度 IPI
```

调度流程：

```text
tick / yield / block / sleep / exit / reschedule
  -> scheduler 锁内同步当前任务快照
  -> 更新 TaskState 和 ready/running CPU 归属
  -> 将任务放入 ready、wait、sleep 或 exited 容器
  -> 选择本 CPU 的下一个 runnable task；没有任务时选择物理 idle task
  -> 更新 CPU current 和任务 Running 状态
  -> 释放 scheduler lock 与中断 guard
  -> 锁外发送 IPI，并在需要时执行 __switch
```

任务从就绪队列被选中运行时会先出队，再标记为 Running。因此一个任务不能同时位于两个 CPU
的就绪队列，也不能在 Running 的同时仍留在就绪队列。

## PCB实现功能

PCB 与 ProcessRegistry 的主要实现在 `task-impl/impl-core/src/process.rs`。

- 保存 PID、父进程、leader task、地址空间引用和进程内线程表。
- 使用 `processes`、`pid_for_task`、`task_for_thread` 三个 `BTreeMap` 维护 PCB 主表以及
  PID/TID/TaskId 反向索引。
- 支持 `fork` 创建子进程，以及 `clone` 将新线程登记到现有进程。
- 维护进程组 PGID、会话 SID、rlimit、umask、dumpable、child subreaper 和
  parent-death signal。
- 维护进程的 Running、Stopped、Exiting、Exited 状态以及 stop/continue 的 wait 可见事件。
- `exec` 期间通过 `exec_in_progress` 阻止并发注册 member，并协助清理同进程其它线程。
- 非 leader 线程退出后进入 `exited_member_task_ids`，回收路径可以按 ID 精确处理，不需要每次
  扫描整个线程表。
- 进程完全退出后暂时保留 PCB，使父进程能够通过 `wait*` 获取退出信息；reap 时再移除 PCB、
  三组索引并在锁外释放地址空间。
- 修改 registry 的可失败接口使用 `ProcessResult<T>` 区分进程不存在、任务不存在、权限不足和
  参数非法；仅查询且“不存在”属于正常结果时仍使用 `Option<T>`。

## TCB实现功能

TCB 的主要实现在 `task-impl/impl-core/src/tcb.rs`。

- 区分 Idle、Kernel、User 三种任务资源。三者都拥有独立内核栈和任务上下文；用户任务额外
  拥有用户 trap frame、入口、用户栈和地址空间信息。
- 维护 `Ready`、`Running`、`Blocking`、`Sleeping`、`Exited` 五种调度生命周期状态。
- 维护调度策略、实时优先级、nice、vruntime、运行统计和等待结果。
- `ready_cpu_id` 表示 Ready 任务所属的唯一 CPU runqueue；`running_cpu_id` 表示任务当前运行
  的 CPU；`last_cpu_id` 用于唤醒时尽量回到最近运行 CPU，提高缓存局部性。
- 新创建但尚未发布的任务状态也是 Ready，但 `ready_cpu_id` 为 `None`，且不在任何就绪队列；
  调用 `start_user_task`、`start_fork_child` 或 `start_clone_thread` 后才正式入队。
- 支持从父 TCB 构造 fork 子任务：复制用户现场和调度属性，安装独立地址空间，并令子任务的
  syscall 返回值为 0。
- 支持 clone 同进程线程：共享进程地址空间，创建独立内核栈、上下文和 trap frame，并可设置
  用户栈及 TLS。
- 支持 exec：替换用户入口、栈、trap frame 和地址空间信息，同时保留当前调度实体。
- 提供 `TaskSnapshot`，供调度器、dashboard 和 GDB 诊断在锁外读取稳定状态。

## 调度器实现功能

调度器的详细说明见 [`task-scheduler/readme.md`](task-scheduler/readme.md)。核心功能包括：

- `CfsQueue` 使用 `BTreeMap<VRunTime, VecDeque<TaskId>>`：按最小 vruntime 选择任务，同一
  vruntime 下保持 FIFO。nice 越小权重越大，每 tick 增加的 vruntime 越少，因而获得更多 CPU
  份额；任务跨 CPU 入队时会按目标 CPU 的最小 vruntime 做归一化，避免低值插队。
- B-tree 与红黑树的有序操作复杂度均为 `O(log n)`。当前采用标准库 `BTreeMap`，可以直接获得
  有序首项并避免侵入式树节点；但按 TaskId 任意删除仍需要扫描 vruntime 桶，后续可增加
  `TaskId -> VRunTime` 反向索引优化。
- FIFO 和 RR 使用 1 到 99 的 priority 分桶，按 99 到 1 选择；相同 priority 下 FIFO 先于
  RR。FIFO 不因普通时间片轮转，RR 达到时间片后重新调度。
- 调度类别顺序为：实时 FIFO/RR → Other/Batch 公平类 → `SCHED_IDLE` → 每 CPU 物理 idle
  task。Other 与 Batch 比较跨队列最小 vruntime，相等时 Other 优先。
- 新任务投递到符合 affinity 的最空 online CPU，负载相同时使用轮转起点避免长期偏向 CPU 0；
  唤醒任务优先尝试 `last_cpu_id`，目标过载或离线时重新选择 CPU。
- 远端入队只对实际目标 CPU 设置 `need_resched` 并发送定向 IPI；IPI 处理重调度请求但不推进
  scheduler 时间。
- BSP 指定的 timekeeper CPU 是唯一推进 sleep/wait timeout 的 CPU；其它 CPU timer 只推进
  本地运行统计、fair vruntime、RR 时间片和抢占判断。
- WaitQueues 支持条件等待、超时、wake-one、wake-all 和 requeue。条件等待在 scheduler
  临界区内复查条件，避免条件变化与入队之间发生丢失唤醒。
- `cpu_states()`、`task_snapshot()` 和 `log_stall_diagnostics()` 可用于检查 CPU online、当前
  任务、各类队列长度、等待目标、`need_resched`、timer 和 context-switch 是否继续推进。

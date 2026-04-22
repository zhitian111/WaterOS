# wateros-task 公共 API 快照

## 当前定位

当前已具备 `task-api`、`task-impl`、`task-scheduler` 的拆分结构，且 `task-impl/impl-dummy` 已开始承载真实任务对象。

聚合层当前对外暴露的核心能力包括：

- 任务创建：`spawn_kernel_task`
- 启动首个任务：`run_first_task`
- 主动让出：`yield_now`
- 时钟驱动调度：`schedule_tick`
- 阻塞与睡眠：`block_current`、`sleep_for_ticks`
- wait queue：`WaitQueue::new`、`wait_current`、`wake_one`、`wake_all`
- 唤醒与退出：`wake_task`、`exit_current`
- 当前任务查询：`current_task_id`、`current_task_snapshot`

`task-api` 当前已补齐 `TaskKind`、`TaskState`、`TaskBlockReason`、`TaskExitCode`、`TaskTick`、`TaskTrapFrame`、`TaskSnapshot`、`ScheduleReason` 等第二阶段基础语义，并且已经收紧为纯任务语义层：公共 `KernelTask` / `TaskSnapshot` 不再暴露 `TaskContext`。同时，任务启动协议也开始从“裸 `entry + arg`”收束为 `KernelTaskStart` 描述对象，trap 路径也开始把完整 trap frame 快照写回当前任务对象，并在返回前从任务对象回写到 trap 栈帧。寄存器级切换上下文现在只保留在 `task-impl` 与 `task-scheduler` 的机制路径中，并由 `platform-arch` 提供当前架构实现；scheduler 内部的数据结构也开始从“队列直接持有任务对象”收敛为“中央 task 表 + TaskId 队列”。`task-scheduler` 则在第一阶段 round-robin 的基础上补入了阻塞队列、睡眠队列、退出队列、最小 trap frame 同步能力以及最小 `WaitQueue` 支持。

## 事实来源

- 组件根 `Cargo.toml`
- 组件聚合 `src/lib.rs`
- 对应 `api-v0` 与 `impl-*` 目录

## 维护要求

当聚合层导出项、默认 feature 或组件边界发生变化时，应同步更新本文件。

# wateros-task 公共 API 快照

## 当前定位

当前已具备 `task-api`、`task-impl`、`task-scheduler` 的拆分结构，且 `task-impl/impl-dummy` 已开始承载真实任务对象。

聚合层当前对外暴露的核心能力包括：

- 任务创建：`spawn_kernel_task`
- 启动首个任务：`run_first_task`
- 主动让出：`yield_now`
- 时钟驱动调度：`schedule_tick`
- 阻塞与睡眠：`block_current`、`sleep_for_ticks`
- 唤醒与退出：`wake_task`、`exit_current`
- 当前任务查询：`current_task_id`、`current_task_snapshot`

`task-api` 当前已补齐 `TaskKind`、`TaskState`、`TaskBlockReason`、`TaskExitCode`、`TaskTick`、`TaskSnapshot`、`ScheduleReason` 等第二阶段基础语义；其中 `TaskContext` 现在通过 `platform-arch` 提供的 `ArchTaskContext` 对外暴露。`task-scheduler` 则在第一阶段 round-robin 的基础上补入了阻塞队列、睡眠队列和退出队列的最小状态流转。

## 事实来源

- 组件根 `Cargo.toml`
- 组件聚合 `src/lib.rs`
- 对应 `api-v0` 与 `impl-*` 目录

## 维护要求

当聚合层导出项、默认 feature 或组件边界发生变化时，应同步更新本文件。

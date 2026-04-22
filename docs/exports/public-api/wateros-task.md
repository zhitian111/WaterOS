# wateros-task 公共 API 快照

## 当前定位

当前已具备 `task-api`、`task-impl`、`task-scheduler` 的拆分结构，且 Stage3A 已完成第一轮边界收紧：根 crate 更偏 facade，任务启动与 trap hook 已迁入内部 runtime，`task-impl/impl-dummy` 则继续承载真实任务对象。

聚合层当前对外暴露的核心能力包括：

- 任务创建：`spawn_kernel_task`
- 启动首个任务：`run_first_task`
- 主动让出：`yield_now`
- 时钟驱动调度：`schedule_tick`
- 阻塞与睡眠：`block_current`、`sleep_for_ticks`
- 通用等待：`wait_on`、`wait_on_for_ticks`、`task_exit_wait_handle`
- wait queue：`WaitQueue::new`、`wait_current`、`wait_current_for_ticks`、`wake_one`、`wake_all`
- 任务退出等待：`wait_for_task_exit`、`wait_for_task_exit_for_ticks`
- 唤醒与退出：`wake_task`、`exit_current`
- 退出回收：`reap_exited_task`、`reap_one_exited_task`
- 当前任务查询：`current_task_id`、`current_task_snapshot`

`task-api` 当前已补齐 `TaskKind`、`TaskState`、`TaskBlockReason`、`TaskExitCode`、`TaskTick`、`TaskTrapFrame`、`TaskSnapshot`、`ScheduleReason` 等基础语义，并进一步收紧为稳定任务视图层：`TaskSnapshot` 现在是显式快照结构，只暴露 `id`、`kind`、`state`、`stats` 和最近一次 trap frame 快照，不再暴露内核栈顶、启动入口或 bootstrap 协议细节。任务启动协议对象已经从公共 API 中移出，转而留在 `task-impl` 与 `wateros-task` runtime 的内部机制路径中。寄存器级切换上下文仍只保留在 `task-impl` 与 `task-scheduler` 的机制层，并由 `platform-arch` 提供当前架构实现；`task-scheduler` 内部则进一步收敛为“任务注册表 + round-robin 队列”结构，并补出 `TaskWaitHandle` / `TaskWaitTarget` 这一层，把 waitqueue、任务退出等待和 timeout 统一到了同一条等待路径上，退出任务也会以 zombie 形式保留直到被显式回收。`TaskTrapFrame` 现已显式提供 `user_pc`、`user_sp`、`set_syscall_ret`、`set_return_to_user`、`prepare_user_return` 等 helper，用来表达“当前任务 trap 现场在恢复时将返回到用户态”的正式语义，而不再只依赖零散的寄存器位操作。

## 事实来源

- 组件根 `Cargo.toml`
- 组件聚合 `src/lib.rs`
- 对应 `api-v0` 与 `impl-*` 目录

## 维护要求

当聚合层导出项、默认 feature 或组件边界发生变化时，应同步更新本文件。

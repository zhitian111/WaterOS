# wateros-task 公共 API 快照

## 当前定位

当前已具备 `task-api`、`task-impl`、`task-scheduler` 的拆分结构，且 Stage3A 已完成第一轮边界收紧：根 crate 更偏 facade，任务启动与 trap hook 已迁入内部 runtime，`task-impl/impl-core` 继续承载真实任务对象，而 `task-scheduler/scheduler-impl/impl-round-robin` 则承载当前主线调度策略。

## 聚合层导出（`wateros-task` 根 `lib.rs`）

| 项 | 说明 |
|----|------|
| **`pub mod api`** | **`pub use api_v0::*`**（含 **`TaskRuntimeStats`** 等；部分类型同时在根 **`pub use api_v0::{...}`** 显式列出）。 |
| **`pub mod scheduler`** | **`pub use scheduler::*`**（调度器门面、**`ScheduleReason`**、**`Scheduler`** trait、**`TaskTrapFrame`**、**`active_impl`** 等，见 **`wateros-task-scheduler`**）。 |
| **`active_impl`** | 仅 **`#[cfg(feature = "impl-core")]`**：**`impl_core`**（**`TaskBootstrap`**、**`TaskControlBlock`** 等）。 |
| **`WaitQueue`** | 根上定义的 **`struct WaitQueue`** 及 **`new` / `id` / `wait_handle` / `wait_current` / `wait_current_for_ticks` / `wake_one` / `wake_all`**，内部委托 **`scheduler`**。 |
| **根 `pub use api_v0::{...}`** | **`AddressSpaceHandle`**、**`ExitedTask`**、**`KernelTaskEntry`**、**`TaskBlockReason`**、**`TaskExitCode`**、**`TaskId`**、**`TaskKind`**、**`TaskSnapshot`**、**`TaskState`**、**`TaskTick`**、**`TaskTrapSnapshot`**、**`TaskWaitHandle`**、**`TaskWaitResult`**、**`TaskWaitTarget`**、**`UserImageInfo`**、**`UserTaskEntryPc`**、**`UserTaskResources`**、**`UserTaskSpec`**、**`WaitQueueId`**、**`IDLE_TASK_ID`**。 |
| **根级函数** | **`init`**、**`init_kernel_trap_satp`**、**`spawn_kernel_task`**、**`spawn_user_task_spec`**、**`spawn_user_task`**、**`run_first_task`**、**`yield_now`**、**`schedule_tick`**、**`block_current`**、**`wait_on`**、**`wait_on_for_ticks`**、**`task_exit_wait_handle`**、**`wait_for_task_exit`**、**`wait_for_task_exit_for_ticks`**、**`sleep_for_ticks`**、**`wake_task`**、**`reap_exited_task`**、**`reap_one_exited_task`**、**`exit_current`**、**`current_task_id`**、**`current_task_snapshot`**。 |

更多调度器独有入口（如 **`current_task_address_space_raw`**、trap frame 存取）仅暴露在 **`scheduler`** 模块路径下，根层不重复包装。

聚合层当前对外暴露的核心能力包括：

- 任务创建：`spawn_kernel_task`
- 用户任务骨架创建：`spawn_user_task`
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

`task-api` 当前已补齐 `TaskKind`、`TaskState`、`TaskBlockReason`、`TaskExitCode`、`TaskTick`、`TaskTrapSnapshot`、`TaskSnapshot` 等基础语义，并进一步收紧为稳定任务视图层：`TaskSnapshot` 现在是显式快照结构，只暴露 `id`、`kind`、`state`、`stats` 和最近一次 trap 语义快照，不再暴露内核栈顶、启动入口、bootstrap 协议细节或完整架构 trap frame 布局。任务启动协议对象已经从公共 API 中移出，转而留在 `task-impl` 与 `wateros-task` runtime 的内部机制路径中。寄存器级切换上下文和完整 trap frame 仍只保留在 `task-impl`、`task-scheduler` 与 runtime 机制层，并由 `platform-arch` 提供当前架构实现；`task-scheduler` 内部则进一步收敛为“任务注册表 + round-robin 队列”结构，并补出 `TaskWaitHandle` / `TaskWaitTarget` 这一层，把 waitqueue、任务退出等待和 timeout 统一到了同一条等待路径上，退出任务也会以 zombie 形式保留直到被显式回收。调度原因 `ScheduleReason` 已从 task 公共契约移动到 scheduler API，trap frame record/restore hook 也不再属于 `Scheduler` 公共 trait。Stage3C-4 当前还补上了最小的 `spawn_user_task(entry_pc)` 骨架：任务对象会持有独立用户栈，并准备好首次 `sret` 所需的 trap frame，但完整用户态执行、用户栈切换和后续 trap 生命周期仍待继续完善。

## 事实来源

- 组件根 `Cargo.toml`
- 组件聚合 `src/lib.rs`
- 对应 `api-v0` 与 `impl-*` 目录

## 维护要求

当聚合层导出项、默认 feature 或组件边界发生变化时，应同步更新本文件。

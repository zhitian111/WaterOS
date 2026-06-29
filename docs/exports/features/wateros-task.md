# wateros-task — 已实现功能快照

## 用途

记录 `wateros-task` 一级组件当前已落地能力、feature 组合与已知缺口。事实来源：`os/components/wateros-task/**` 源码与 `Cargo.toml`。

## 子 crate 与职责

| 子 crate | 职责 | 状态 |
|----------|------|------|
| `wateros-task`（聚合） | 组合 API、调度器、进程 registry；对上暴露 spawn/wait/fork/exec 等 | 已实现 |
| `wateros-task-api-v0` | 任务/进程/等待/调度语义类型与轻量构造 | 已实现 |
| `wateros-task-scheduler` | 调度算法聚合；`active_impl` 转发 | 已实现 |
| `scheduler-api/api-v0` | `Scheduler`/`SwitchScheduler` trait、TCB 注册表、等待队列 | 已实现 |
| `scheduler-impl/impl-multi-class` | OTHER + FIFO + RR 多类就绪队列（默认） | 已实现 |
| `scheduler-impl/impl-round-robin` | 单 `SCHED_OTHER` 轮转（互斥 feature） | 已实现 |
| `wateros-task-impl-core` | `TaskControlBlock`、内核/用户栈、进程 registry | 已实现（`impl-core` feature） |

## Feature 矩阵（聚合层）

| Feature | 效果 |
|---------|------|
| `api-v0` | 启用 API 与调度器契约 |
| `impl-core` | 链接 TCB 与进程 registry（默认开启） |
| `impl-multi-class` | 多类调度实现（默认） |
| `impl-round-robin` | 纯轮转实现；与 `impl-multi-class` 互斥 |

## 已实现能力

### 调度与生命周期

- **内核/用户任务创建**：`spawn_kernel_task`、`spawn_user_task`、ELF 装载辅助 `spawn_user_task_from_loaded_elf`。
- **上下文切换**：经 arch `__switch`；idle 任务 `wfi`；首次切入 `run_first_task`。
- **主动让出 / tick**：`yield_now`、`schedule_tick`；轮转时间片由 `config::task::MAX_TICKS_PER_TASK` 控制。
- **阻塞与等待**：显式 wait queue、任务退出等待、子进程退出等待、睡眠、超时、条件等待（`wait_on_while*`）。
- **退出与回收**：`exit_current`、`exit_group_current`、`reap_exited_task`、`reap_exited_process`、`purge_all_user_processes`。

### 进程语义（registry）

- **PID/TID 分配**：leader 的 tid 与 pid 同号；clone 线程独立 tid。
- **fork / clone / execve**：与 MM 地址空间协作；失败回滚 `abort_fork_child` / `abort_clone_thread`。
- **wait 族语义**：stopped/continued 子进程查询、`SIGSTOP`/`SIGCONT` 进程级阻塞与恢复。
- **资源限制**：`RLIMIT_*` 读写；`nice`、进程组 `pgid`、会话 `setsid` 子集。
- **bring-up 清理**：`purge_all_user_processes` 强制结束孤儿进程。

### Trap 与运行时

- **C ABI 入口**：`__wateros_task_runtime_entry`、`__wateros_idle_task_runtime_main`、`__wateros_task_runtime_enter_current_user_task`。
- **Trap 帧委托**：`begin_current_trap_frame_access` / `restore_current_trap_frame` 在用户任务与 arch trap handler 间同步。

### 调度策略（syscall 层）

- **`sched_*` 原语**：`sched` 模块提供 `get_scheduler`/`set_scheduler`/`set_affinity` 等；单核 affinity 恒为 CPU0。
- **多类调度**（`impl-multi-class`）：FIFO/RR 队列骨架存在；bring-up 有效策略仍为 `SCHED_OTHER`。

## 与相邻组件的分工

| 组件 | 分工 |
|------|------|
| `wateros-mm` | ELF 装载、用户地址空间、fork COW；task 持有 token/指针 |
| `wateros-platform-arch` | `__switch`、trap frame、`TaskContext` |
| `wateros-abi` | 用户返回值写入 trap 帧（fork/clone 子返回 0） |
| 根 `syscall` | 映射 Linux errno；调用 `task::` 聚合 API |

## 缺口与后续

- **SMP**：全局 `UniprocessorSafeCell` + 关中断临界区，无 per-CPU run-queue。
- **RT 调度**：`SCHED_FIFO`/`SCHED_RR` 队列存在但 bring-up 未完整抢占语义。
- **资源句柄**：`file_table`/`cwd`/`signal_handlers` 等为 `ResourceHandle` 占位，由 syscall 层外挂。
- **`impl-round-robin`**：策略变更仅更新 TCB，不迁移 run-queue（与 multi-class 行为不同）。

## 修订

| 日期 | 说明 |
|------|------|
| 2026-06-29 | 初版导出（注释/inline 任务同步） |

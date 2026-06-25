# wateros-task 功能快照

## 事实来源

- `os/components/wateros-task/Cargo.toml`、`src/lib.rs`
- `task-api/api-v0`、`task-impl/impl-core`、`task-scheduler/`
- `os/src/main.rs`（`task::init`、`init_kernel_trap_satp`、`run_first_task`）
- `os/src/self_tests/task.rs`（固定 hello world ELF 启动与 pipe IPC 自检）

## 当前状态

当前已具备单核内核态任务切换、timer 驱动的多类调度（`impl-multi-class` 为默认 `active_impl`），以及 Stage3A 第一轮边界收紧后的任务/runtime/scheduler 分层。

当前已落地的能力包括：

- 任务对象由 `task-impl/impl-core` 统一承载
- `TaskSnapshot` 已收敛为稳定公共快照，不再暴露栈顶地址和启动协议细节
- 任务状态已从单纯 `Ready/Running` 扩展为 `Ready`、`Running`、`Blocking`、`Sleeping`、`Exited`
- 调度器已可区分 `yield`、timer tick、阻塞、睡眠与退出等调度原因
- 调度器已开始收敛为“任务注册表 + TaskId 队列”，并具备最小的阻塞队列、睡眠队列、退出队列和显式唤醒入口
- 已具备最小 `WaitQueue` 能力，可显式 `wait_current`、`wait_current_for_ticks`、`wake_one`、`wake_all`
- 已具备条件等待能力：`wait_on_while` / `wait_on_while_for_ticks` 与 `WaitQueue::wait_current_while*` 会在调度临界区内复查条件，服务 pipe 等 IPC 对象的无丢唤醒等待
- 已具备最小的 timed wait 与退出回收入口，可显式 `reap_exited_task`、`reap_one_exited_task`
- 已引入通用 `TaskWaitHandle` / `TaskWaitTarget`，`waitqueue`、“等待任务退出”与“等待任意子任务退出”已共用同一条等待与 timeout 路径
- spawn 会记录最小 `parent_id`，`TaskSnapshot` / `ExitedTask` 暴露该关系，供 syscall `waitpid` 判断与回收子任务
- 退出任务现在会保留为可回收 zombie，并在退出时自动唤醒等待其退出的 waiter 或父任务的 child-exit waiter
- 调度器采用多类架构：`OtherReadyQueue`（`SCHED_OTHER` 轮转）、`RtFifoRunQueue`（`SCHED_FIFO`）、`RtRrRunQueue`（`SCHED_RR`）；`pick_next_runnable` 按 RT 最高优先级（同优先级 FIFO 先于 RR）→ OTHER → IDLE 选取
- TCB 含 `sched_policy` / `sched_priority`；`set_scheduler` / `set_param` 经 `apply_sched_policy_change` 从旧类 run-queue detach、更新 TCB、按新策略入队，必要时 `RescheduleNow` 抢占
- `impl-round-robin` 保留为 OTHER 参考实现，可通过 feature 回退；共享 `TaskRegistry`、`WaitQueues`、`OtherReadyQueue` 位于 `scheduler-api`
- 调度器在 `QueueTarget::Exited` 路径上**先** `wake_all_waiters_for_task_exit`、**再** `detach_task_from_run_queues`；若顺序颠倒，`detach` 会 `remove(exit_wait_queues[task_id])`，导致 `wait_for_task_exit` 的 waiter 永久丢失（bringup runner 等路径会卡死）
- task 根 crate 已收紧为 facade，trap/tick/task-entry hook 已迁入内部 runtime
- trap 路径已开始把完整 trap frame 快照复制进当前任务对象，并在返回前回写到 trap 栈帧
- trap 读写路径已显式区分“是否返回用户态”的语义，完整 trap frame 留在 `platform-arch`/task impl 机制层，task 公共 API 通过 `TaskTrapSnapshot` 暴露架构无关语义快照
- 已具备最小 `spawn_user_task` 骨架：用户任务可预分配用户栈，并准备首次 `sret` 进入所需的 trap frame
- 已接入 `wateros-mm-api-v0::kernel_bringup::LoadedElf`：task 根 crate 可将 MM loader 返回的 `entry_pc`、`satp`、镜像范围与外部栈区间转换为 `UserTaskSpec`，并直接生成用户态任务
- RISC-V 自检已以根卷默认 ELF 作为唯一用户态回归路径；ELF observer 会等待、reap 并校验退出码、trap frame、地址空间、image 与外部栈元数据
- LoongArch64 路径已用独立 `.text.user_smoke` 段创建 PLV3 用户态 syscall smoke，并通过 `UserTaskSpec` / observer 校验 entry、image、栈与 trap frame 快照；它目前不声明地址空间句柄，真实 ELF 任务仍依赖后续 LoongArch MM/FS/loader 接入
- `current_task_snapshot` 可提供不含任务切换上下文、但包含最近一次 trap 语义快照的轻量任务状态快照与统计信息

## 后续关注点

- 继续把当前“复制 + 回写”模式推进为完整 trap frame 归属与恢复模型
- 继续把当前 wait handle 与条件等待模型推进为更完整的通用阻塞对象 / block object 层
- 继续补更明确的 task handle / generation 语义，并在 fork/exec 完成后收敛完整父子进程生命周期
- 继续扩展真实用户态镜像覆盖面，包括更多 syscall 与进程/地址空间场景
- 持续补齐注释与公共 API 文档

## fork/clone 实现说明

### 当前实现

`sys_clone` → `task::fork_current` → `TaskControlBlock::fork_from`，并由 `mm::kernel_mm::fork_user_aspace` 为子进程提供**独立页表**（含用户栈页复制）。

| 参数 | 用户 SP | 地址空间 |
|------|---------|----------|
| `child_stack != 0`（clone） | 设为 `child_stack` | 独立（`fork_user_aspace`） |
| `child_stack == 0`（fork） | 保留父进程 fork 瞬间 trap 帧中的 `user_sp` | 独立（`fork_user_aspace`） |

子 trap 帧：`a0 = 0`，`sepc` 跳过已完成的 `ecall`，`satp` 指向新页表。

#### 继承

- `vfs::cwd::copy_cwd_from_parent`、`vfs::fd::copy_fd_table_from_parent` 在 `sys_clone` 中完成。

#### 已知限制

- **无 COW**：`fork_user_aspace` 为可写用户页做逐页复制，大工作集时 fork 开销较高；尚未实现写时复制。
- **clone 标志位**：`clone(2)` 的 `flags`（线程组、TLS 等）尚未解析。

## 锁机制审计（2026-06-25）

**2026-06-25 修复轮**：调度器 wait 释中断、页缓存驱逐重入、帧分配器/klog 关中断、pipe Mutex、shm attach 占位、unix 清理与 bind 锁序、procfs 锁外回调、paged truncate 路由、aux RO 双实例拒绝。详见 [`docs/audits/lock-issues.md`](../../audits/lock-issues.md) §9。

调度器与 `ProcessRegistry` 使用 `UniprocessorSafeCell` + `InterruptGuard`；Spawn/Fork/Clone 与 Registry 登记非原子（**PR-01 暂缓**）。详见 [`docs/audits/locks/scheduler.md`](../../audits/locks/scheduler.md)、[`process-registry.md`](../../audits/locks/process-registry.md)。

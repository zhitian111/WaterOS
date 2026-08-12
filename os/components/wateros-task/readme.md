# wateros-task

[项目首页](../../../README.md) · [内核工程](../../README.md) · [系统架构](../../../README.md#系统架构)

`wateros-task` 是 WaterOS 的任务与进程生命周期门面。它把 syscall、trap、MM 与 IPC 所需的
操作汇聚到稳定接口，并将“任务/进程对象”和“何时、在哪个 CPU 上运行”的职责分别交给
`task-impl` 与 `task-scheduler`。

## 分层与边界

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 聚合门面 | `src/lib.rs` | 对外重导出任务、进程、调度与等待接口；初始化 task 和 scheduler。 |
| 领域 API | `task-api/api-v0/` | `TaskId`、`TaskState`、process 快照、clone/scheduler 参数等稳定类型。 |
| 任务实现 | `task-impl/impl-core/` | TCB、内核/用户任务上下文、任务/进程 registry 与运行时资源。 |
| 调度器 | `task-scheduler/` | ready queue、CPU-local 状态、wait queue、上下文切换与 SMP 放置。 |
| 上层调用方 | syscall、trap、MM、IPC | ABI、用户内存、signal frame、地址空间创建和 IPC 对象语义。 |

```text
syscall / trap / IPC
  │ fork / clone / exec / exit / wait / sleep / wake
  ▼
wateros-task facade
  ├─ task-impl: TCB、进程关系、用户/内核上下文和退出资源
  └─ task-scheduler: TaskState、runqueue、waitqueue、CPU 与 __switch
```

顶层 crate 不应保存第二份任务状态。需要新增状态时，先确定它属于：任务本身、进程共享对象、
CPU-local scheduler 状态，还是某个 IPC 对象；不要为了方便塞进 facade 的静态变量。

## 核心标识与状态

| 类型 | 归属 | 说明 |
| --- | --- | --- |
| `TaskId` | task registry | 内核任务唯一标识；不是 Linux PID/TID。 |
| `ProcessId` / `ThreadId` | process 领域 | 用户可见进程/线程身份；一进程可对应多个任务。 |
| `TaskState` | scheduler + TCB | `Ready`、`Running`、`Blocking`、`Sleeping`、`Exited`。 |
| `SchedPolicy` | scheduler | `Other`、`Batch`、`Idle`、`Fifo`、`Rr` 五种策略；队列规则见 `task-scheduler` 文档。 |
| `TaskWaitTarget` / `WaitQueueId` | scheduler | 描述阻塞原因和具体等待队列。 |
| `CpuId` / `CpuMask` | scheduler / platform | 配置 CPU 身份与亲和性集合。 |
| `ProcessSnapshot` / `TaskSnapshot` | 查询接口 | 面向诊断、procfs、wait 和 dashboard 的复制快照，不是可修改引用。 |

任务状态、队列归属与运行 CPU 必须一致：

- `Ready` 只属于一个 CPU 的一个 ready queue；
- `Running` 的 `running_cpu_id` 只能与该 CPU 的 `current_task_id` 对应；
- `Blocking(WaitQueue(id))` 必须与 scheduler 的 waiter 索引一致；
- `Exited` 不能重新入队，后续只允许 wait/reap 路径读取其退出信息。

这些关联由 scheduler 锁内的状态转换维持。业务代码不要直接修改 `TaskState` 或猜测任务在哪个
runqueue；使用 `wake_task`、`interrupt_task`、`yield_now`、`block_current` 等门面。

## 生命周期

```text
创建 kernel/user task ──> Ready ──> Running
                                   │   │
                          wake <──┘   ├─ yield / tick / reschedule ─> Ready
                                      ├─ wait / futex / pipe ───────> Blocking
                                      ├─ sleep ─────────────────────> Sleeping
                                      └─ exit / signal terminate ───> Exited ─> reap

fork:  创建新 process/task，再由 MM、signal、FD 等子系统完成各自复制
clone: 同 process 新 thread/task，继承或共享资源依 clone flags 决定
exec:  终止同进程其余线程，替换当前用户镜像，并重置 exec-sensitive 资源
```

`fork_current`、`clone_current_thread`、`execve_current`、`exit_current` 等只负责 task
领域的阶段；syscall 调用者必须按既定顺序协调 MM、signal、futex robust、FD 与 process
资源。跨层回调必须在各自锁释放后进行。

## 等待、唤醒与 IPC

`wait_queue::WaitQueue` 是对 scheduler wait queue 的轻量句柄。IPC 对象应使用
`wait_on_while` / `wait_on_while_for_ticks` 或 `ipc-waitqueue` 的同等接口：条件会在
scheduler 临界区复查，避免“先检查、后睡眠”造成 lost wake。

```text
对象状态更新（对象锁内） -> 释放对象锁 -> wake_task / WaitQueue::wake_*
当前任务等待            -> wait_on_while(condition) -> 条件仍成立才 Blocking
```

不得持有 IPC、VFS、MM 或 process 锁进入等待、`yield`、`sleep`、`exit` 或 `__switch`。

## SMP 规则

- 每个 online CPU 有独立 current task、idle task 和 runnable 队列；任务的 `ready_cpu`、
  `last_cpu`、`running_cpu` 由 scheduler 更新。
- 新任务通常投向最空的 online CPU；唤醒任务优先回到 `last_cpu`，不可用时再选目标 CPU。
- 远端入队后 scheduler 记录重调度请求，并在锁外向**实际目标 CPU**发送 IPI；IPC/业务模块
  不自行广播。
- 只有 timekeeper CPU 推进全局 sleep/wait timeout；其他 CPU timer 只处理本地时间片和抢占，
  防止多核将 timeout 加速。
- 用户地址空间的 active CPU 维护和 TLB shootdown 属于 MM；调度切换通知 MM 进入/离开地址空间。

## 常用入口

- 启动：`init()`、`run_first_task()`；AP 则在 CPU-local、trap、页表与中断准备后调用
  `run_first_task_on_current_cpu`（scheduler 导出）。
- 创建：`spawn_kernel_task`、`create_user_task`、`spawn_user_task`。
- 调度：`yield_now`、`schedule_tick`、`schedule_reschedule`、`sleep_for_ticks`。
- 等待：`wait_on*`、`wait_for_task_exit*`、`wake_task`、`interrupt_task`。
- 进程：`fork_current`、`clone_current_thread`、`execve_current`、`exit_current`、
  `reap_exited_process`。
- 可观测性：`task_snapshot`、`process_snapshot`、`cpu_snapshot`、`cpu_states`、
  `print_cpu_states`、`log_stall_diagnostics`。

## 验证与排障

```sh
make -C os rv_check
make -C os la_check
```

排查卡死时，首先对照：CPU snapshot 的 `current_task_id` / `need_resched`、各 CPU ready queue
长度、任务 `TaskState`、以及对应 `WaitQueueId`。若发现 `Ready` 任务没有 queue 归属，或同一
任务同时显示为多个 CPU 的 current，优先检查 scheduler 锁内的状态转换路径。

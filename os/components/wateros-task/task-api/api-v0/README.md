# Task API v0 开发手册

[Task 总览](../../README.md) · [离线开发手册](../../../../docs/offline-development/README.md)

本 crate 定义任务、进程、调度和等待的跨实现数据契约，不持有全局 registry，也不实现队列或上下文切换。新增字段时要先明确它属于线程、进程还是一次快照；放错层级通常会在 clone/fork/exec 后产生语义错误。

## 类型地图

| 文件 | 类型 | 语义 |
| --- | --- | --- |
| `task.rs` | `TaskId`、`TaskState`、`TaskKind`、`ExitedTask`、运行统计 | 一个可调度执行流及其 scheduler 状态。 |
| `process.rs` | PID/TID、`CloneFlags`、`AddressSpaceRef`、PCB/线程快照、rlimit/caps | Linux 风格进程与线程组的公共视图。 |
| `sched.rs` | `SchedPolicy`、nice、priority、vruntime、`SchedError` | OTHER/BATCH/IDLE/FIFO/RR 属性范围。 |
| `snapshot.rs` | `TaskSnapshot`、`TaskTrapSnapshot` | 调度与诊断使用的值快照，不能当可变 TCB。 |
| `user.rs` | `UserTask`、`UserImageInfo`、地址空间/栈/入口包装 | MM/装载器交给 task 的用户映像规格。 |
| `kernel.rs` | `KernelStack`、`TaskBootstrap` | 内核/idle 任务初次切入所需资源。 |
| `wait.rs` | `TaskWaitTarget`、`TaskWaitResult` | 阻塞原因与 Woken/TimedOut/Interrupted 结果。 |

## 两套身份与状态

`TaskId` 是内核调度实体 ID；`ThreadId` 是用户 ABI 可见 TID；`ProcessId` 是线程组 PID。不要用数值恰好相同推导身份，必须通过 `ProcessRegistry` 的索引查询。leader 的 PID/TID 规则由创建逻辑维持，不是 newtype 自动保证。

`TaskState` 决定任务位于 ready/running/wait/exited 哪个调度容器；`ProcessState` 决定 wait 可见的 running/stopped/exiting/exited 语义；`ProcessTaskState` 是进程 registry 对组内线程的视图。一次 exit 往往要更新不止一个状态机。

## 所有权与快照规则

- `AddressSpaceHandle`/`AddressSpaceRef` 只是句柄和共享关系描述，不拥有页表；最终销毁由 process 生命周期通过 MM hook 完成。
- `TaskSnapshot.task_cx` 等地址只适合锁内准备切换和诊断，不可跨任务回收长期保存。
- `UserTask` 必须含有效地址空间和用户栈才能构造真实用户 TCB；trap 返回 token 与 MM handle 是相关但用途不同的值。
- `KernelStack` Drop 会释放栈。bootstrap 指针和上下文 SP 指向其内部时，栈与 bootstrap 必须留在 TCB 中共同存活。
- `CloneFlags` 只表达 task 层已支持的子集。新增 flag 需同步 syscall 参数检查、资源共享/复制、失败回滚和测试，不能只放宽 bitmask。

## 新增进程/调度 syscall 的模板

以新增查询/设置线程属性为例：

1. 在这里定义架构无关类型、范围和错误；判断属性是 per-thread 还是 process-wide。
2. 在 `impl-core` 的 TCB 或 PCB 添加唯一真相字段，并在 fork/clone/exec 写清继承规则。
3. 在 scheduler 锁或 process registry 锁内提供原子查询/更新；若影响 ready queue，先摘队、改属性、再按新策略唯一入队。
4. 在 `wateros-task/src` 暴露协调接口，syscall 只做 ABI/errno 转换。
5. 覆盖当前线程、其它线程、无效 PID/TID、并发退出、fork/clone/exec 继承和 SMP 运行中更新。

## 回归不变量

- 一个 `Ready` 任务至多属于一个 CPU 的一个 ready queue；新建但未发布时允许 `ready_cpu_id=None`。
- `Running` 任务恰有一个 `running_cpu_id`，且不在任何 ready/wait 队列。
- `ExitedTask` 被 reap 后，任何 PID/TID/TaskId 反向索引不能继续指向已释放 TCB。
- wait 返回结果必须区分正常唤醒、超时和中断；不能把虚假唤醒直接解释成条件成立。
- API 变更需同时编译聚合层、impl-core、scheduler-api 和 multi-class 实现。


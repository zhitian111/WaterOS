# ipc-signal

[项目首页](../../../../README.md) · [内核工程](../../../README.md) · [wateros-ipc](../readme.md)

`ipc-signal` 维护 Linux 风格的进程级和线程级信号状态。它负责 disposition、mask、pending、
备用信号栈和 timer 状态机，但不直接调度任务，也不读写用户态 signal frame。

## 模块分层

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 聚合层 | `src/lib.rs` | 重导出版本化 API 和 core 实现，不保存全局状态。 |
| Signal API | `signal-api/api-v0/` | 定义信号常量、位集、action、timer、dispatch 和错误。 |
| Signal 实现 | `signal-impl/impl-core/` | 维护 registry、进程/线程状态、投递选择和 timer。 |
| 系统调用层 | `wateros-syscall/.../sys/ipc/signal.rs` | 处理 Linux ABI、用户内存、权限和 task 副作用。 |
| Trap/架构层 | `wateros-platform` 与 syscall signal frame | 保存、构造和恢复用户 signal frame。 |

实现文件按职责拆分如下：

| 文件 | 内容 |
| --- | --- |
| `signal-api/api-v0/src/lib.rs` | 稳定类型、常量、SignalSet、SignalAction 和 SignalDispatch。 |
| `signal-impl/impl-core/src/state.rs` | 仅实现层可见的进程、线程和 timer 状态。 |
| `signal-impl/impl-core/src/registry.rs` | SignalRegistry、生命周期、mask、pending 和投递选择。 |
| `signal-impl/impl-core/src/timer.rs` | interval、POSIX 和 CPU timer 计算。 |
| `signal-impl/impl-core/src/global.rs` | 全局 registry 锁与模块级 facade。 |

## 实现说明

- 进程共享 disposition、process pending、interval/POSIX timer 和 CPU timer 统计。
- 每个线程独立保存 blocked mask、thread pending、临时等待 mask、sigwait 集合和备用信号栈。
- 普通信号使用 64 位 `SignalSet` 保存 pending，同一种普通信号最多保留一个 pending 位，不累计
  重复投递次数。
- `SIGKILL` 和 `SIGSTOP` 不能被屏蔽，也不能安装自定义 handler。
- signal registry 锁只保护信号状态和跨表不变量，不得在锁内进入 scheduler、waitqueue、VFS、
  用户内存或 IPI 路径。
- `SignalDispatch` 是锁内计算出的待执行意图，不表示目标任务已经被唤醒、停止、继续或终止。
- syscall/task 层必须在 signal API 返回并释放锁后应用 dispatch。
- 用户 signal frame 的复制、PC/SP 设置和 `rt_sigreturn` 恢复由 syscall 与架构层负责。

## 调用链路

信号投递和交付流程：

```text
kill / tkill / terminal / timer
  -> SignalRegistry::send_process / send_thread
  -> 更新 process/thread pending，并选择候选线程
  -> 返回 SignalDispatch
  -> 锁外唤醒、停止、继续或终止目标 task
  -> trap 返回用户态前 take_deliverable(task_id)
  -> 安装 handler mask，并返回 SignalEffect
  -> syscall 层把用户现场写入 signal frame
  -> arch 设置 PC/SP/参数寄存器进入 handler
  -> handler 经 trampoline 调用 rt_sigreturn
  -> 恢复用户现场、原 mask 和备用栈状态
```

生命周期流程：

```text
fork
  -> 复制 disposition、调用线程 mask 和备用栈
  -> 不继承 pending、interval timer 或 POSIX timer

CLONE_THREAD
  -> 共享进程 SignalState
  -> 新线程继承 mask，不继承备用栈

exec
  -> 保留 ignored disposition、pending 和 interval timer
  -> 重置用户 handler、POSIX timer 与备用栈

thread/process exit
  -> 删除线程状态
  -> 最后一个线程退出后删除进程信号状态
```

timer 流程：

```text
setitimer / timer_settime
  -> registry 更新 timer state 和 deadline
  -> 全局时间路径调用 expire_realtime / expire_posix_timers
  -> 生成 SignalDispatch 列表
  -> 调用方锁外投递和应用 task 副作用
```

## ProcessSignalState实现功能

进程级内部状态定义在 `signal-impl/impl-core/src/state.rs`。

- 保存 PID 对应的 signal disposition 表和 process pending 位集。
- 保存该进程包含的线程及其 TID 顺序，用于选择最低 TID 的可接收线程。
- 保存 `ITIMER_REAL`、`ITIMER_VIRTUAL`、`ITIMER_PROF` 和 POSIX timer 状态。
- 维护 CPU timer 所需的用户态/内核态累计时间。
- fork、exec 和最后线程退出时，按 Linux 语义复制、重置或删除进程级状态。

进程级 pending 属于整个进程。`send_process` 会优先选择未屏蔽该信号或正在 sigwait 的合适
线程，但 pending 的最终可交付判断仍在目标线程的安全点完成。

## ThreadSignalState实现功能

线程级内部状态同样定义在 `signal-impl/impl-core/src/state.rs`。

- 保存 task ID、用户 TID、所属 PID、blocked mask 和 thread pending。
- 保存 `sigsuspend`、`ppoll`、`pselect6` 使用的 temporary restore mask。
- 保存 `sigwait` 等待集合，使进程投递可以优先选择同步等待者。
- 保存 alternate signal stack 配置以及当前是否位于该栈上的状态。
- handler 开始时安装 action mask，`rt_sigreturn` 时恢复原 mask。
- temporary restore mask 同一时刻只能属于一种临时等待，所有结束和异常路径都必须清理。

task ID 是 WaterOS 内部调度标识，TID 是用户可见线程编号，PID 是进程编号，三者不能混用。

## SignalRegistry实现功能

`SignalRegistry` 定义在 `signal-impl/impl-core/src/registry.rs`，由 `global.rs` 中唯一的
`REGISTRY` 锁保护。

- `processes` 保存 `PID -> ProcessSignalState`。
- `threads` 保存 `TaskId -> ThreadSignalState`。
- `real_deadlines` 按单调时间保存 `(PID, generation)`，用于快速查找到期的 ITIMER_REAL。
- 保证每个已注册线程都指向一个已注册进程，并协调 fork/clone/exec/exit 的跨表更新。
- 支持进程投递、线程投递、pending 查询、mask 更新、action 和备用栈操作。
- `take_deliverable` 在安全点结合 pending、mask 和 disposition 生成 Default/Ignore/Handler 等
  SignalEffect。
- timer deadline 使用 generation 过滤 setitimer 替换后残留的旧条目。

全局 facade 位于 `signal-impl/impl-core/src/global.rs`。调用方应使用模块级函数，不直接取得
SignalRegistry 或从其锁保护区回调 signal 模块。

## SignalTimer实现功能

timer 逻辑主要位于 `signal-impl/impl-core/src/timer.rs`。

- `ITIMER_REAL` 使用单调时间 deadline，到期后产生通常为 `SIGALRM` 的 dispatch。
- `ITIMER_VIRTUAL` 只累计用户态 CPU 时间。
- `ITIMER_PROF` 累计用户态加内核态 CPU 时间。
- POSIX timer 根据选择的时钟维护 deadline、interval、signal 和 overrun。
- 调用者只能对实际运行的进程记 CPU 时间；多 CPU 同时运行同进程线程时必须分别累计真实
  消耗，不能用全局 wall tick 代替。
- 到期函数只更新 signal 状态并返回 dispatch，不在 registry 锁内操作 task。

## Signal聚合层实现功能

`ipc-signal/src/lib.rs` 只负责导出 API 和 `impl-core`：

- 对外提供 SignalSet、SignalAction、SignalEffect、SignalDispatch、timer 类型和信号常量。
- 重导出进程/线程注册、投递、mask、pending、signal-frame 状态和 timer facade。
- 调用方应通过 `ipc::signal` 使用领域接口；Linux 用户结构转换和 errno 映射仍留在 syscall 层。

host 单元测试覆盖 pending 合并、fork/clone/exec、mask 恢复、备用栈及 interval/POSIX/CPU
timer。排障时首先区分“pending 已写入”“dispatch 已返回”和“调用方已应用 task 副作用”这三个
阶段，避免把未执行的 SignalDispatch 误认为 scheduler 故障。

# wateros-ipc

[项目首页](../../../README.md) · [内核工程](../../README.md) · [系统架构](../../../README.md#系统架构)

`wateros-ipc` 是 WaterOS 的进程间通信聚合模块。它统一导出 waitqueue、futex、pipe、SysV
共享内存和 signal，并把各 IPC 对象的状态管理与 syscall ABI、用户地址空间和 task scheduler
分开。

## 模块分层

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 聚合门面 | `src/lib.rs` | 按 feature 导出 IPC API、实现和各子模块。 |
| 通用 API | `ipc-api/api-v0/` | 保留版本化 IPC 公共契约；当前仍是占位门面。 |
| 等待适配 | `ipc-waitqueue/` | 把 IPC 条件等待统一委托给 `wateros-task` scheduler。 |
| Futex | `ipc-futex/` | 管理 futex key、waiter、requeue 和 robust-list 登记。 |
| Pipe | `ipc-pipe/` | 实现匿名 pipe 的 ring buffer、端点生命周期和阻塞 I/O。 |
| 共享内存 | `ipc-shm/` | 管理 SysV SHM 段、物理页、attachment 和延迟删除。 |
| Signal | `ipc-signal/` | 管理进程/线程信号状态、pending、mask、disposition 和 timer。 |
| 系统调用层 | `wateros-syscall/.../sys/ipc/` | 解析 Linux ABI、访问用户内存、映射 errno，并协调 MM/task 副作用。 |

各子模块继续采用“聚合 crate → API crate → 实现 crate”的结构：

| 子模块 | API | 当前实现 | 详细文档 |
| --- | --- | --- | --- |
| waitqueue | `waitqueue-api/api-v0/` | `waitqueue-impl/impl-task/` | [`ipc-waitqueue`](ipc-waitqueue/readme.md) |
| futex | `futex-api/api-v0/` | `futex-impl/impl-task/` | [`ipc-futex`](ipc-futex/readme.md) |
| pipe | `pipe-api/api-v0/` | `pipe-impl/impl-ringbuf/` | [`ipc-pipe`](ipc-pipe/readme.md) |
| shm | `shm-api/api-v0/` | `shm-impl/impl-frame/` | [`ipc-shm`](ipc-shm/readme.md) |
| signal | `signal-api/api-v0/` | `signal-impl/impl-core/` | [`ipc-signal`](ipc-signal/readme.md) |

## 实现说明

- IPC 子模块负责内核对象及其并发状态，不直接解析 syscall 号，也不直接把领域错误转换为
  Linux errno。
- syscall 层负责用户指针读写、Linux 结构布局、权限检查和 ABI 兼容；MM 层负责用户页表与
  TLB；task 层负责阻塞、唤醒、timeout、CPU 放置和重调度 IPI。
- waitqueue 是 IPC 与 scheduler 之间唯一的等待适配层。futex 和 pipe 不维护第二套任务状态，
  也不自行选择被唤醒任务应运行在哪个 CPU。
- futex registry、pipe state、SHM registry 和 signal registry 各自使用独立锁保护。对象锁只
  保护该对象的内存状态，不能代替 scheduler、process 或 address-space 锁。
- 任何可能阻塞、调度、访问用户内存、修改页表或发送跨核通知的操作，都不应在持有 IPC
  registry/object 锁时执行。
- 默认 feature 只提供版本化门面和 waitqueue；内核双架构 feature 通过
  `ipc/all` 启用 futex、pipe、shm 和 signal。
- `ipc-event/` 当前只是未接入 workspace 和聚合层的占位目录，不属于现有可用 IPC 接口。

## 调用链路

条件等待的公共链路：

```text
IPC 对象锁内检查/更新对象状态
  -> 释放对象锁
  -> ipc-waitqueue::WaitQueue::wait_current_while(condition)
  -> scheduler 锁内再次检查 condition
  -> 条件仍成立时把当前任务改为 Blocking 并切走
  -> wake 后重新放入合适 CPU 的就绪队列
```

在 scheduler 临界区复查条件，是为了封住“调用方第一次判断应等待”和“任务真正登记为
waiter”之间的 lost-wake 窗口。

Futex 链路：

```text
sys_futex
  -> 校验操作码并读取用户 futex 字
  -> 构造 private/shared FutexKey
  -> ipc::futex 更新 registry 与 wake_sequence
  -> ipc-waitqueue 阻塞、唤醒或 requeue task
  -> syscall 层把 FutexWaitOutcome/FutexError 转成返回值或 errno
```

Pipe 链路：

```text
pipe2 / read / write / poll
  -> fd-session 持有 PipeEndpoint
  -> PipeState ring buffer 尝试读写
  -> 空/满且对端仍存在时，经 read_wait/write_wait 条件等待
  -> 数据变化或最后一个端点关闭后，在锁外唤醒对侧
```

共享内存链路：

```text
shmget
  -> SHM registry 创建/查询段并分配物理页

shmat
  -> begin_attach 建立 reservation 并取得页快照
  -> 释放 SHM 锁，由 MM 选择用户 VA 并映射页面
  -> 成功调用 finish_attach；失败调用 cancel_attach_reservation

shmdt / task exit / IPC_RMID
  -> 更新 attachment 或删除 key 索引
  -> 最后一个 attachment 消失后才释放已标记删除段的物理页
```

Signal 链路：

```text
kill / tkill / timer 到期
  -> signal registry 更新 process/thread pending
  -> 返回 SignalDispatch
  -> 释放 signal 锁后，syscall/task 层执行唤醒、停止、继续或终止
  -> trap 返回用户态前 take_deliverable
  -> syscall/arch 构造 signal frame 并进入 handler
  -> rt_sigreturn 恢复用户现场和线程 signal mask
```

## WaitQueue实现功能

`ipc-waitqueue` 是对 `wateros_task::wait_queue::WaitQueue` 的薄包装，不保存第二套 waiter
容器。

- 创建和释放 scheduler 管理的 `WaitQueueId`。
- 支持当前任务等待、按条件等待、带 tick timeout 的等待、wake-one 和 wake-all。
- 支持 futex requeue 所需的等待者原子迁移。
- 将 wait result、Blocking 状态、timeout 和被唤醒任务的 CPU 放置交给 task scheduler。
- 条件闭包在 scheduler 临界区执行，只应进行短小、不可阻塞的状态检查。
- `try_release_empty` 只能在上层确认没有 waiter、也没有并发使用者持有旧 ID 时调用。

因此，pipe/futex 等模块只负责决定“条件是否仍成立”，不负责直接修改 TaskState、ready queue
或发送 IPI。

## Futex实现功能

futex 的主要实现在 `ipc-futex/futex-impl/impl-task/src/`。

- `FutexKey` 区分进程私有 futex 和基于共享物理身份的 shared futex。
- `FutexRegistry` 维护 key 到 `FutexQueue` 的映射、任务到等待 key 的反向索引及 robust-list
  登记。
- `wait_while` 使用用户条件复查与 `wake_sequence`，防止 futex 字变化或并发 wake 后任务重新
  睡入旧队列。
- `wake`、`wake_all`、`requeue` 和 `cmp_requeue` 在 registry 锁外执行真正的 waitqueue 操作。
- `active_users` 覆盖取得队列句柄到锁外操作完成的窗口，防止空队列 ID 被提前释放并复用。
- robust-list 只保存线程登记；线程退出时由 syscall 层遍历用户链表、写入
  `FUTEX_OWNER_DIED` 并唤醒 waiter。
- 当前不支持 PI futex；bitset 操作仅支持 `FUTEX_BITSET_MATCH_ALL`。

## Pipe实现功能

pipe 的主要实现在 `ipc-pipe/pipe-impl/impl-ringbuf/src/`。

- `PipeState` 保存固定容量环形字节缓冲、head、有效长度和读写端引用状态。
- `PipeEndpoint` 区分读端与写端，并由 fd-session 作为文件句柄持有。
- 空 pipe 且写端存在时读者等待 `read_wait`；满 pipe 且读端存在时写者等待
  `write_wait`。
- 空 pipe 且写端全部关闭时读取返回 EOF；读端全部关闭时写入返回 `BrokenPipe`，上层可据此
  产生 `SIGPIPE`。
- 非阻塞端点在暂时不能读写时返回 `WouldBlock`；poll readiness 根据缓冲和对端状态计算。
- 显式 close 与 Drop 共享幂等关闭状态，确保每个端点引用至多释放一次。
- 当前不保证 Linux `PIPE_BUF` 原子写入，也未实现 `O_DIRECT` packet mode。

## SHM实现功能

共享内存的主要实现在 `ipc-shm/shm-impl/impl-frame/src/`。

- `ShmRegistry` 使用段主表、SysV key 索引和 task attachment 表管理共享段。
- 创建段时按页对齐分配并清零物理帧；这些帧始终由 SHM registry 持有。
- `begin_attach`、`finish_attach` 和 `cancel_attach_reservation` 构成两阶段 attach，避免在持有
  registry 锁时进入 MM。
- `IPC_RMID` 先删除 key 可见性并标记段，已存在映射继续有效；只有 `nattch == 0` 后才回收
  物理页。
- fork 可以复制 attachment 元数据；实际页表复制、失败回滚和 TLB shootdown 仍由调用方/MM
  负责。
- 当前单段上限为 4 MiB，尚未实现完整 `shmctl` 权限、huge page 和 memory policy。

## Signal实现功能

signal 的主要实现在 `ipc-signal/signal-impl/impl-core/src/`。

- `SignalRegistry` 分别维护进程级 disposition/pending/timer 和线程级 mask/pending/wait/备用栈
  状态。
- 支持进程信号、线程定向信号、普通 pending 合并、mask、`sigaction`、`sigsuspend`、
  `sigwait` 和临时 poll mask。
- 支持 `ITIMER_REAL`、`ITIMER_VIRTUAL`、`ITIMER_PROF` 及当前 POSIX timer 状态和到期处理。
- 按 fork、clone-thread、exec 和 exit 语义复制、重置或删除进程/线程信号状态。
- `SignalDispatch` 只描述锁内计算出的调度意图；真正的 task 唤醒、停止、继续、终止和 IPI
  必须由调用方在 signal 锁外执行。
- signal frame 不属于 IPC registry：用户现场保存、handler 参数安装和 `rt_sigreturn` 由
  syscall 与架构 trap 层完成。

## IPC聚合层实现功能

顶层 `src/lib.rs` 不保存 futex、pipe、SHM 或 signal 的全局状态，只负责模块选择与重导出：

- `ipc::waitqueue` 始终提供 IPC 等待适配接口。
- 启用相应 feature 后，分别通过 `ipc::futex`、`ipc::pipe`、`ipc::shm` 和 `ipc::signal`
  访问子模块。
- `api` 保留版本化门面；真实领域 API 位于各子模块中。
- syscall、VFS 和 task 等调用方应使用顶层重导出，不应跨过聚合层依赖具体实现 crate。

扩展新的 IPC 对象时，应继续保持三条边界：Linux ABI 和用户复制留在 syscall 层；对象状态由
独立 IPC 锁保护；所有可能等待、调度、修改页表或跨核通知的操作都在释放对象锁后执行。

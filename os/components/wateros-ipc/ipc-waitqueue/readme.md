# ipc-waitqueue

[项目首页](../../../../README.md) · [内核工程](../../../README.md) · [wateros-ipc](../readme.md)

`ipc-waitqueue` 是 IPC 对 WaterOS task scheduler 等待队列的唯一适配层。它不保存第二套
waiter 列表，也不实现自己的 timeout、CPU 选择或 IPI。

## 模块分层

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 聚合层 | `src/lib.rs` | 选择 task 实现并导出 WaitQueue 与公共类型。 |
| WaitQueue API | `waitqueue-api/api-v0/` | 定义 IpcWaitQueueOps 和 task 等待类型契约。 |
| WaitQueue 实现 | `waitqueue-impl/impl-task/` | 薄包装 `wateros_task::wait_queue::WaitQueue`。 |
| Scheduler | `wateros-task/task-scheduler/` | 保存 waiter、处理 timeout、状态转换、CPU 放置和 IPI。 |
| IPC 调用方 | futex、pipe 及其它阻塞对象 | 提供对象条件，并在对象锁外执行等待或唤醒。 |

## 实现说明

- IPC WaitQueue 与 scheduler WaitQueue 共用同一个 WaitQueueId 和 waiter 容器。
- 当前任务进入等待后，TaskState 由 scheduler 改为 `Blocking(WaitQueue(id))`。
- wake 后的 Ready 状态、ready_cpu、目标 runqueue 和远端重调度 IPI 都由 scheduler 更新。
- timeout 使用 scheduler 的全局 TaskTick 和 timekeeper CPU，不受在线 CPU 数量影响。
- IPC 对象只提供“是否仍需等待”的条件，不直接修改 TaskState 或运行队列。
- 对象状态锁不能跨越 wait/wake/requeue；条件闭包会在 scheduler 临界区短暂重新取得对象状态。
- 应优先使用条件等待，裸 `wait_current` 只适合不存在外部条件竞态的特殊路径。

## 调用链路

条件等待流程：

```text
IPC 对象锁内检查自身状态
  -> 判断当前应等待
  -> 释放对象锁
  -> WaitQueue::wait_current_while(condition)
  -> scheduler 锁内调用 condition 再次检查
  -> 条件已改变：直接返回，不阻塞
  -> 条件仍成立：登记 waiter，设置 Blocking，并调度其它任务
  -> wake / timeout / signal 后恢复并返回 TaskWaitResult
```

唤醒流程：

```text
IPC 对象锁内修改数据、端点或 sequence
  -> 释放对象锁
  -> WaitQueue::wake_one / wake_all
  -> scheduler 核对 waiter 的 TaskState 和等待目标
  -> 将有效任务重新发布到合适 CPU
  -> 锁外向实际远端目标发送定向 IPI
```

requeue 流程：

```text
source.requeue_to(target, wake_count, requeue_count)
  -> scheduler 同一临界区唤醒部分 source waiter
  -> 将其余 waiter 移入 target
  -> 同步修改 TCB 的 Blocking target
```

## WaitQueue实现功能

`WaitQueue` 的 IPC 包装实现在 `waitqueue-impl/impl-task/src/lib.rs`。

- 创建 scheduler 管理的 WaitQueueId，并持有该队列的上层句柄。
- 提供 `wait_current`、条件等待和带 tick deadline 的等待。
- 提供 wake-one、wake-all、查询 waiter 和 requeue 操作。
- 把 TaskId、TaskTick、TaskWaitResult、TaskWaitTarget 和 WaitQueueId 统一重导出给 IPC 调用方。
- Drop/显式释放必须尊重 scheduler 队列生命周期，不能让仍被并发使用的 ID 提前复用。

WaitQueue 句柄表示一个调度器等待对象，不表示拥有其中 Task；任务生命周期仍归 TaskRegistry
和 scheduler 管理。

## 条件等待实现功能

- `wait_current_while` 在 scheduler 临界区复查条件，消除“第一次状态检查”和“登记 waiter”之间
  的 lost-wake 窗口。
- `wait_current_while_for_ticks` 在相同条件语义上增加 scheduler deadline。
- 条件闭包不能睡眠、调度、访问用户内存或取得会反向进入 scheduler 的锁。
- wake 时 scheduler 会丢弃状态或等待目标已经不匹配的陈旧 waiter，避免任务重复 Ready。
- futex 等还需要 wake sequence 处理用户条件复查后的额外竞态；该协议属于 futex 层，不应
  塞入通用 WaitQueue。

## WaitQueue生命周期实现功能

- `try_release_empty` 只证明 scheduler 当前没有 waiter，不能证明上层没有并发句柄。
- 静态、长期存在的 pipe 等待队列通常随 Pipe 一起销毁。
- futex 这类按 key 动态创建的队列必须额外维护 active-users，覆盖查表到锁外操作完成的窗口。
- 只有“无 waiter + 无并发使用者”同时成立时，动态 WaitQueueId 才能安全释放和复用。

## WaitQueue聚合层实现功能

`ipc-waitqueue/src/lib.rs` 只负责 API 与实现选择：

- 对外导出 IpcWaitQueueOps、WaitQueue 和 task 等待公共类型。
- 具体实现始终委托 `wateros-task`，不建立独立全局 registry。
- IPC 子模块应通过 `ipc::waitqueue` 或自身依赖的聚合 crate 使用接口，不直接操作 scheduler
  内部 WaitQueues。

排查等待卡住时，应把 IPC 对象条件、WaitQueueId、TCB 的 Blocking target、deadline 和目标
CPU 的 ready 状态放在同一条链路上检查，不能只观察 IPC 对象内部是否调用过 wake。

## 失败边界与回归

条件闭包阻塞/用户fault、持对象锁wait、提前复用ID或active owner遗漏都会造成死锁/误唤醒。回归覆盖检查—登记窗口、wake与timeout同tick、signal/exit、requeue和远端CPU wake，并确认释放后的stale Copy不再使用。

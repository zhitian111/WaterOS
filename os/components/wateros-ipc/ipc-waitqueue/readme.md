# ipc-waitqueue

`ipc-waitqueue` 是 IPC 对 task scheduler 等待队列的唯一适配层。它不保存第二套 waiter
列表，也不实现自己的 timeout、CPU 选择或 IPI；所有这些语义都委托 `wateros-task`。

## 分层

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 聚合 | `src/lib.rs` | 重导出稳定 API 与选中的 task 实现。 |
| API | `waitqueue-api/api-v0/` | 等待、条件等待、唤醒及 task 类型契约。 |
| 实现 | `waitqueue-impl/impl-task/` | 薄包装 `wateros_task::wait_queue::WaitQueue`。 |
| 底层 | `wateros-task` scheduler | waiter 状态、全局 timeout、运行队列、SMP 重调度。 |

## 使用规则

```text
IPC 对象更新自身状态（持有对象锁）
  -> 释放对象锁
  -> WaitQueue::wake_one / wake_all

当前任务准备等待
  -> WaitQueue::wait_current_while(condition)
  -> scheduler 临界区再次检查 condition
  -> 条件仍成立才转为 Blocking(WaitQueue(id))
```

- 优先使用 `wait_current_while` / `wait_current_while_for_ticks`，不要先检查条件再调用裸
  `wait_current`，否则会产生 lost wake 窗口。
- `try_release_empty` 仅在上层已确保无人会继续持有该队列 ID 时使用。像 futex 这类可并发查找
  队列的对象，需要额外 `active_users` 生命周期保护。
- `requeue_to` 用于 futex 一类原子迁移；不要在 IPC 自己的锁中实现第二套 waiter 搬运逻辑。

## SMP 边界

waitqueue 调用 scheduler 的统一接口。被唤醒任务会由 scheduler 在同一把调度锁内更新状态、
队列归属和 `ready_cpu`，必要时只向实际目标 CPU 发重调度 IPI。IPC 模块不能自行广播 IPI，也
不能在持有对象锁时进入等待/调度路径。

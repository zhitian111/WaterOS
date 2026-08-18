# Task-backed IPC WaitQueue 手册

[IPC 总览](../../../README.md) · [Task 调度器](../../../../wateros-task/task-scheduler/README.md)

本实现是 `wateros_task::WaitQueue` 的零额外状态、`Clone + Copy` 包装。它只保存一个底层 `WaitQueueId`；waiter、task state、timeout 与远端 runqueue 的唯一真相都在 task scheduler。IPC 对象可维护 futex key/bitset 等业务索引，但不能建立第二份“谁处于 Blocking”的权威表。

## 接口分组

- 身份：`new/new_named`、`id`、`wait_target`、`try_release_empty`；
- 等待：`wait_current`、`wait_current_for_ticks`；
- 防丢唤醒等待：`wait_current_while[_for_ticks]`；
- 唤醒：`wake_one`、`wake_all`、`wake_task`；
- futex 搬迁：`requeue_to`、条件版本和 detailed 条件版本。

所有方法直接委托 task waitqueue；IPC wrapper 不附加锁，也不改变 target CPU/IPI 决策。`IpcWaitQueueOps` trait 实现与固有方法应保持逐项一致，新增接口时两处都要补。

正确阻塞模式是：对象锁内检查条件，释放对象锁，再调用 `wait_current_while[_for_ticks]`，由 condition 在 scheduler 临界区短暂重取对象锁复查。这样封住“首次检查后、任务登记前”发生 wake 的窗口。condition 必须短、不可阻塞，不能做用户拷贝、MM、VFS 或 IPI。

```text
对象操作发现需等待
  -> 保存足以再次判断的对象/sequence
  -> 释放对象锁
  -> wait_current_while(|| 条件仍不满足)
  -> scheduler 原子登记 Blocking 并切换
  -> wake/timeout/interrupt 将任务恢复 Ready
```

`try_release_empty` 只说明 scheduler 队列此刻无 waiter；futex 等还有锁外操作者时，必须用自己的 `active_users` 延迟释放 ID。requeue 的 detailed 版本返回 scheduler 实际验证并移动的 TaskId，外部 registry 应以此同步自己的 waiter key，不能只依据请求数量。

## Copy 与 ID 生命周期陷阱

`WaitQueue` 可 Copy，是为了把同一 ID 传入短调用和 requeue target，不表示每个副本独立拥有队列。任一副本 `try_release_empty()` 成功后，所有旧副本都成为逻辑 stale handle；若底层复用 ID，旧副本再 wake 可能误伤新对象的 waiter。

因此业务对象需要额外 owner/lifetime 规则：只有 registry 中唯一的销毁者，在阻止新操作者、`active_users==0` 且 queue empty 时才 release；release 后整个对象不可再公开。当前类型没有 generation 字段，无法自行检测 stale copy。

`wake_task(task_id)` 直接调用全局 `wateros_task::wake_task`，不会验证 task 正在本 queue 等待。注释所说的“已由上层 registry 确认”是硬前提。futex bitset 使用它时要在同一业务同步协议下验证 key/queue/bitset，不能拿陈旧 TaskId 任意唤醒。

## timeout、interrupt 与返回值

等待返回 `TaskWaitResult`，调用者必须区分 wake、timeout、signal interrupt 等原因，然后再次检查业务条件。wake 和 timeout 同 tick 时只能有一个 scheduler 状态转换胜出，但业务上仍以条件为准；被唤醒不保证资源归当前任务。

timeout 单位是 task tick，不是纳秒。syscall 层负责从 timespec 换算并处理 absolute/relative clock；零 tick 的行为要按具体调用测试。等待期间对象必须由 Arc/registry active reference 保活，不能只保存裸 queue copy。

## requeue 事务

普通 `requeue_to` 返回总处理数，外部 futex registry 若还保存 waiter key，不能据请求数量猜哪些 TaskId 被移走。优先使用 `requeue_to_detailed_while`：condition 在 scheduler 临界区内复查 compare 值，返回调度器实际 wake/requeue 的集合，再同步业务元数据。

source==target、wake_count/requeue_count 溢出式大值、并发 timeout/exit 都必须由底层安全处理；业务 registry 的锁顺序要固定，跨两个 futex bucket 时按稳定 key 排序，避免 A→B/B→A 死锁。condition 不能试图获取与当前 scheduler 路径反向的锁。

## 新阻塞 IPC 实例

实现消息队列 receive 时：对象锁内查消息；若空，记录 queue owner并解锁；调用 `wait_current_while(|| 短锁复查仍为空且未删除)`；返回后循环。send 在提交消息后解锁，再 wake_one。删除先标记 removed、阻止新 active user，再 wake_all；最后等 active_users 归零且 queue empty 后 release ID。

## 回归清单

- wake 发生在首次检查与 scheduler 复查之间，不得永睡；
- spurious wake 后循环、wake-one/all 数量和 FIFO/公平性现状；
- timeout 同 tick、signal interrupt、task exit 自动摘 waiter；
- 远端 CPU wake 的 runqueue/IPI 与 online race；
- requeue 部分 wake/部分移动、source==target、目标删除；
- detailed TaskId 与业务 registry 完全一致；
- active user 尚在时 release 被拒绝；release 后 stale Copy 不再使用；
- ID 高频创建/释放/复用不唤醒错误对象。

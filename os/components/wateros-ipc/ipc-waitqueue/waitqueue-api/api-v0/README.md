# ipc-waitqueue API v0 离线开发手册

本文描述 IPC 等待队列的稳定接口。实现细节见
[impl-task](../../waitqueue-impl/impl-task/README.md)，模块总览见
[ipc-waitqueue](../../readme.md)，调度语义见
[wateros-task](../../../../wateros-task/README.md)。源码始终是最终依据。

## 1. 模块边界

本 crate 只完成两件事：

- 重导出 task 子系统的等待类型；
- 定义不同等待队列实现必须满足的 `IpcWaitQueueOps` trait。

它不保存 waiter，不选择唤醒 CPU，不发送 IPI，也不拥有 timeout。当前
`impl-task` 是 `wateros_task::WaitQueue` 的零额外状态包装，IPC 层不能再维护一份
waiter 链表，否则两份状态很容易在超时、信号和任务退出时失步。

## 2. 公开数据类型

| 类型 | 含义 | 所有者 |
| --- | --- | --- |
| `TaskId` | 任务编号 | task |
| `TaskTick` | 调度 tick 计数 | task/timer |
| `WaitQueueId` | task 子系统中的等待队列编号 | task |
| `TaskWaitTarget` | 等待目标；本模块使用 `WaitQueue(id)` | task |
| `TaskWaitResult` | `Woken`、`TimedOut` 或 `Interrupted` | task |

特别注意：`TaskWaitResult` 没有“条件已经变化”变体。`wait_current_while` 的闭包
返回 `false` 时，底层不会睡眠，但 API 仍以正常返回表示。若上层必须区分“真正被
wake”与“入睡前条件已失效”，应在自己的状态机中记录序号或重新检查条件，futex
的 `FutexWaitOutcome::ConditionChanged` 就是在 futex 层产生的，不是 task 层结果。

## 3. `IpcWaitQueueOps` 接口

| 方法 | 语义 | 关键约束 |
| --- | --- | --- |
| `new` / `new_named` | 分配队列 ID | 名称仅用于诊断 |
| `id` | 取得底层队列编号 | 只用于标识，不能推断生命周期 |
| `wait_target` | 取得 `TaskWaitTarget` | 可交给任务等待/诊断接口 |
| `try_release_empty` | 空队列时回收 ID | 仅在无并发外部引用时调用 |
| `wait_current` | 当前任务无限等待 | 单独使用时要自行封闭 lost-wake 窗口 |
| `wait_current_for_ticks` | 带相对 tick 超时等待 | 返回结果必须处理超时和中断 |
| `wait_current_while` | 调度临界区内复查后等待 | 推荐的条件等待入口 |
| `wait_current_while_for_ticks` | 条件等待并带超时 | 条件闭包必须短且不阻塞 |
| `wake_one` | 唤醒一个 waiter | 返回实际被唤醒的 `TaskId` |
| `wake_all` | 唤醒全部 waiter | 返回实际数量 |
| `requeue_to` | 唤醒前若干，再迁移后若干 | 源、目标生命周期都必须有效 |
| `requeue_to_while` | 条件成立才 requeue | 条件在 scheduler 临界区执行 |

实现 crate 还提供定向 `wake_task` 和返回任务集合的详细 requeue，但它们不是 v0
trait 的可移植契约。通用 IPC 对象不要依赖实现私有扩展。

## 4. 正确的条件等待调用链

典型“队列非空才读”的调用链如下：

```text
消费者持有对象锁检查状态
  -> 当前不可读，释放对象锁
  -> wait_current_while(|| 短暂加对象锁并检查“仍不可读”)
     -> scheduler 临界区内决定入队或立即返回
  -> 醒来后重新从头检查状态

生产者持有对象锁写入状态
  -> 释放对象锁
  -> wake_one / wake_all
```

示意代码：

```rust
loop {
    if let Some(value) = state.lock().pop() {
        return Ok(value);
    }

    match readable.wait_current_while(|| state.lock().is_empty()) {
        TaskWaitResult::Interrupted => return Err(Error::Interrupted),
        TaskWaitResult::Woken | TaskWaitResult::TimedOut => {}
    }
}
```

无限等待通常不会返回 `TimedOut`，但对枚举做穷尽匹配能让代码在实现演进后仍然
显式。醒来不等于条件必然成立：可能有竞争消费者先取走数据，也可能是虚假/广播
唤醒，因此必须循环复查。

## 5. 锁顺序与 lost wake

`wait_current_while` 的条件闭包在 scheduler 临界区内运行，只能做短小、确定、
非阻塞的状态读取。闭包内禁止：

- 访问可能缺页或睡眠的用户内存；
- 调用 VFS、块设备、网络或其它阻塞接口；
- 重入当前等待对象的 wait/wake 路径；
- 长时间计算或等待另一个可能依赖 scheduler 的锁。

调用等待前也不能一直持有闭包会再次取得的对象锁，否则会自死锁。裸
`wait_current()` 只有在“事件不可能在登记前发生”或上层另有 generation/sequence
协议时才安全；一般生产者可能并发运行的条件等待，应使用 `wait_current_while`。

推荐锁顺序是：对象锁只保护业务状态，scheduler 锁只由 waitqueue 内部短暂获取；
更新业务状态后先释放对象锁，再 wake。这样被唤醒者能立即取得对象锁，也减少锁序
环路。

## 6. 生命周期与 ID 回收

当前 `WaitQueue` 是 `Copy` 句柄，复制它不会增加独立引用计数。因而
`try_release_empty()` 的“empty”只证明 scheduler 队列当前没有 waiter，不证明其它
CPU 没有保存同一 ID 的句柄。

只有同时满足以下条件才可回收：

1. 上层对象已经从全局 registry 移除，新的使用者无法取得句柄；
2. 所有锁外 wait/wake/requeue 操作已经结束；
3. 底层等待队列为空；
4. 后续不会再通过旧副本访问该 ID。

过早回收会形成 ABA：旧句柄可能操作已经分配给新对象的相同 ID。futex registry
使用 `active_users` 覆盖锁外操作，就是可复用的生命周期方案。

## 7. requeue 语义

`requeue_to(target, wake_count, requeue_count)` 对源队列执行两步原子调度操作：先唤醒
至多 `wake_count` 个任务，再把至多 `requeue_count` 个剩余任务迁到目标队列。返回值
是实际发生变化的数量，不应假定总等于两个上限之和。

使用 requeue 时，上层还必须同步迁移自己的 waiter 元数据；仅迁移 scheduler 队列
而不更新对象 registry，会造成之后的定向唤醒、清理或统计访问错误对象。源和目标
相同时应在上层拒绝。

## 8. 新增阻塞 IPC 对象实例

以新增 `Semaphore` 为例：

1. 数据结构保存 `Mutex<count>` 和一个 `WaitQueue`，不另建任务链表；
2. `down()` 先在对象锁内尝试减计数；
3. 失败后释放锁，调用 `wait_current_while(|| count == 0)`；
4. 醒来后循环，不把一次 wake 当作资源所有权转移；
5. `up()` 在对象锁内增加计数，释放锁后 `wake_one()`；
6. 若 syscall 可被信号中断，将 `Interrupted` 映射为 `EINTR`；
7. 若支持超时，明确 tick 换算、零超时和 deadline 溢出规则；
8. 对象销毁前先阻止新引用，唤醒或清理 waiter，再安全回收队列 ID。

若对象还维护“已发布但尚未进入 scheduler 队列”的登记状态，应像 futex 一样增加
原子 generation，并在 scheduler 条件闭包中比较 generation，封闭登记到入睡之间的
窗口。

## 9. 常见故障定位

- **偶发永久睡眠**：检查状态复查与 scheduler 入队是否原子，是否误用裸 wait。
- **醒来后读不到数据**：这是允许的竞争结果；若代码没有循环，就是上层错误。
- **SMP 下唤醒延迟**：检查 task scheduler 的目标 CPU、ready queue 和 IPI，不要在
  IPC 层重复发送 IPI。
- **队列 ID 串对象**：检查 `try_release_empty` 是否在仍有副本或锁外操作时调用。
- **条件闭包死锁**：检查调用 wait 时是否仍持有闭包要取得的锁。
- **超时任务仍留在业务表**：task 只清 scheduler waiter；业务 registry 必须在所有
  返回路径执行自己的注销。

## 10. 修改后的自检清单

- 条件等待、生产者更新和 wake 的顺序已经逐项审查；
- `Woken`、`TimedOut`、`Interrupted` 都有明确处理；
- 醒来后总是循环复查业务条件；
- 条件闭包不阻塞、不访问可缺页用户地址；
- 没有在对象锁内进入 scheduler；
- requeue 同时更新 scheduler 与上层元数据；
- 销毁路径证明没有 waiter、没有锁外使用者和旧句柄；
- 单核与 SMP 都测试 wake-one、wake-all、超时、信号中断及销毁竞争；
- 运行 `make check ARCH=rv PROFILE=pre` 和 `make check ARCH=la PROFILE=pre`。


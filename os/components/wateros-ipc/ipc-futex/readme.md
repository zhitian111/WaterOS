# ipc-futex

[项目首页](../../../../README.md) · [内核工程](../../../README.md) · [系统架构](../../../../README.md#系统架构)

`ipc-futex` 是 WaterOS 的 futex IPC 子系统。它把“同一个 futex 字上的任务等待、
唤醒与迁移”映射为 `ipc-waitqueue` 的调度器等待队列，并保存每个线程的 robust-futex
登记状态。

它不解析 Linux `futex(2)` 的裸 ABI，不直接读写用户地址，也不负责把错误转换成
`errno`；这些工作属于 syscall 层。

## 职责与边界

| 层次 | 路径 | 职责 |
|---|---|---|
| 聚合层 | `src/lib.rs` | 重导出当前 API 与实现，不保存状态。 |
| API | `futex-api/api-v0/src/` | 定义 futex key、等待结果、错误和 robust 用户态布局；不依赖 task。 |
| 实现 | `futex-impl/impl-task/src/` | 维护 key 到等待队列的映射、等待者登记、robust 侧表和低频诊断。 |
| 调用方 | `wateros-syscall/.../sys/ipc/` | 解析操作码、校验/读取用户 futex 字、换算超时并映射 errno。 |

```text
sys_futex / robust syscall
  │  读取用户字、构造 FutexKey、提供条件闭包
  ▼
ipc::futex::{wait_while, wake, requeue, cmp_requeue}
  │  管理 FutexRegistry 元数据
  ▼
ipc-waitqueue::WaitQueue
  │  阻塞 / 唤醒 task，必要时由 task scheduler 通知目标 CPU
  ▼
wateros-task scheduler
```

## 核心数据结构

| 类型 | 定义位置 | 所有者 | 关键语义 / 不变量 |
|---|---|---|---|
| `FutexKey` | `futex-api/api-v0/src/key.rs` | syscall 层构造，registry 用作 key | private key 是 `(用户 VA, address-space scope)`；shared key 使用 MM 解析出的稳定共享字身份，`private_scope` 必为 0。 |
| `FutexQueue` | `futex-impl/impl-task/src/registry.rs` | `FutexRegistry::queues` | 一个 key 对应一个 `WaitQueue`、一个 `wake_sequence` 和 `active_users` 引用计数。 |
| `FutexRegistry` | `futex-impl/impl-task/src/registry.rs` | 模块级 `REGISTRY` | 集中保存 `queues`、`waiting_tasks`、`robust` 和诊断计数；仅在 registry 锁内修改。 |
| `waiting_tasks` | `FutexRegistry` 字段 | `FutexRegistry` | `TaskId -> FutexKey`；一个任务最多登记一个 futex 等待。退出、取消和正常返回都必须删除登记并释放队列使用权。 |
| `RobustListRegistration` | `futex-api/api-v0/src/robust.rs` | `FutexRegistry::robust` | 保存线程 robust 链表头、ABI 长度和所属用户地址空间；退出时用 `take_robust_list` 一次性取走。 |
| `FutexWaitOutcome` | `futex-api/api-v0/src/wait.rs` | `wait_while` 返回给 syscall 层 | 区分正常唤醒、条件已改变、超时和信号中断，最终由 syscall 层映射为返回值/errno。 |

数据结构改动时，至少同步检查本表、对应源文件中的类型注释，以及下方的并发不变量；
不要只在 syscall 层补说明。

## 关键流程

### 等待：`wait_while`

实现位置：`futex-impl/impl-task/src/global.rs`。

```text
调用方读取 futex 用户字，确认“仍应等待”
  │
  ├─ 条件不成立 ──────────────────────────────> ConditionChanged
  │
  ▼
registry.acquire_queue(key) + register_waiting_task(task_id, key)
  │  取得 WaitQueue 和 wake_sequence，active_users += 1
  ▼
再次检查用户态条件
  │
  ├─ 条件已改变 ─> finish_waiting_task ───────> ConditionChanged
  │
  ▼
在 WaitQueue 的 scheduler 临界区比较 wake_sequence 后阻塞
  │
  ├─ wake / timeout / signal
  ▼
record_wait_result + finish_waiting_task
  │  删除 waiting_tasks，active_users -= 1，必要时回收空队列
  ▼
FutexWaitOutcome
```

两次 `condition` 检查防止“用户字已改变却仍然睡眠”。第二次检查和真正进入 scheduler
等待之间不再读取用户地址，而是比较 `wake_sequence`：若 wake 正好发生在这个窗口，
序列改变会阻止该任务重新睡回源队列。

### 唤醒：`wake` / `wake_all`

1. 在 registry 锁内取得既有 `FutexQueue`，并增加 `active_users`。
2. 解开 registry 锁后，以 `Release` 递增 `wake_sequence`。
3. 调用 `WaitQueue::wake_one()` 或 `wake_all()`；任务调度器负责把被唤醒任务放回合适的 CPU runqueue。
4. 再进入 registry 锁，记录统计、减少 `active_users`，并在队列已空时回收它。

不存在对应 key、或 `max_wake == 0` 时，`wake` 返回 0，不创建空等待队列。

### 迁移：`requeue` / `cmp_requeue`

`requeue(from_key, to_key, wake_count, requeue_count)` 先唤醒源队列的前若干任务，
再把后续任务迁入目标队列。`cmp_requeue` 在同一 scheduler 临界区通过调用方提供的
条件闭包检查 futex 用户字；不匹配时返回 `FutexError::Again`。

`from_key == to_key` 没有可定义的迁移语义，返回 `FutexError::Invalid`。成功迁移时，
源 `wake_sequence` 会递增，避免尚在“检查条件到真正入睡”窗口内的 waiter 回到已迁移的
源队列。

## 并发与 SMP

### 锁边界

- `REGISTRY: Mutex<FutexRegistry>` 只保护 map、等待者登记、robust 登记和诊断计数。
- **不得**在持有 registry 锁时调用可能阻塞、调度或跨 CPU 通知的 `WaitQueue` 操作；
  `wait_while`、`wake` 和 `requeue` 都在取得队列句柄后释放 registry 锁。
- `WaitQueue` 的内部同步和 task scheduler 锁由 `ipc-waitqueue` / `wateros-task` 管理，
  futex 模块不直接取得 scheduler 锁。

### 不丢 wake 的约束

- `wake_sequence` 由唤醒者在 scheduler wake 前递增，等待者以 `Acquire` 读取。
- `active_users` 覆盖“从 registry 取得 `WaitQueueId` 到锁外 wait/wake/requeue 操作结束”的整个窗口。
- 只有 `active_users == 0` 且 `WaitQueue::try_release_empty()` 成功时，`FutexQueue` 才能从 `queues` 删除。
- 因此，空队列的 `WaitQueueId` 不会在并发操作者仍持有旧句柄时被释放并复用。

### 多核边界

futex registry 本身由同一把自旋锁串行化；被唤醒的任务可能由调度器投递到其他 CPU。
本模块不自行广播 IPI，也不选择目标 CPU。SMP 的任务归属、定向重调度 IPI 与
`ready_cpu_id` 更新均属于 `wateros-task`。

## Robust futex 生命周期

```text
set_robust_list
  -> registry.robust[task_id] = { head, len, user_aspace }

get_robust_list
  -> 查询登记；未登记返回 (0, ROBUST_LIST_HEAD_SIZE)

线程 exit / reap
  -> take_robust_list(task_id)
  -> syscall 层遍历用户链表，写 FUTEX_OWNER_DIED 并唤醒等待者
  -> 已取走的登记不会重复清理
```

`drop_robust_list` 是幂等清理接口，适用于创建失败、reap 回滚和重复清理路径。
IPC 层只保存用户地址和地址空间句柄；实际用户内存访问、坏地址处理以及 PI 节点跳过均由
syscall 层完成。

## 对外接口与错误

主要接口位于 `futex-impl/impl-task/src/global.rs`：

- `wait_while`：条件等待，可带 tick 超时；
- `wake` / `wake_all`：唤醒指定 key 上的等待者；
- `requeue` / `cmp_requeue`：唤醒并迁移等待者；
- `cancel_task_wait`：任务异常终止时撤销等待登记；
- `set_robust_list`、`get_robust_list`、`take_robust_list`、`drop_robust_list`：robust 生命周期；
- `log_debug_snapshot`：低频停滞诊断，不应放入热路径。

`FutexError` 是 IPC 层语义错误：`Again`、`Fault`、`Invalid`、`Nosys`、`TimedOut`、
`Interrupted`。调用方负责映射到 Linux errno；不要在此模块中耦合 syscall 号或用户 ABI。

## 当前限制

- `FUTEX_WAIT_BITSET` / `FUTEX_WAKE_BITSET` 仅支持 `FUTEX_BITSET_MATCH_ALL`。
- PI futex 尚未实现；robust 链表中带 PI 标记的节点由 syscall 层跳过，避免按普通 futex 错误清理。
- shared futex 的正确性依赖 MM 提供稳定共享页/字身份；不能直接把不同地址空间的用户虚拟地址当作 shared key。
- 超时单位是 task scheduler tick，不是 futex 模块自己的时钟；多核下全局 timeout 由 task scheduler 的 timekeeper 语义保证。

## 验证与排障

已有单元测试：

- `futex-api/api-v0/src/key.rs`：private/shared key 作用域；
- `futex-api/api-v0/src/lib.rs`：robust ABI 布局和等待结果枚举；
- `futex-impl/impl-task/src/global.rs`：robust round-trip、缺失队列 wake、相同 key requeue 拒绝。

出现 futex 卡顿时，可在非热路径调用 `log_debug_snapshot()`，并将输出中的
`wait_queue_id` 与 task 状态 `Blocking(WaitQueue(...))` 对照。重点检查：

1. `wait_attempts` 是否持续增加而 `wake_calls` 为 0；
2. `active_users` 是否长期非零；
3. waiter 是否已从 task scheduler 唤醒、但没有完成 `finish_waiting_task`；
4. shared futex 是否错误地使用了用户 VA 而非 MM 解析后的共享身份。

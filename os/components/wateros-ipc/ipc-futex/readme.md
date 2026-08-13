# ipc-futex

[项目首页](../../../../README.md) · [内核工程](../../../README.md) · [wateros-ipc](../readme.md)

`ipc-futex` 是 WaterOS 的 futex IPC 子系统。它把同一个 futex 字上的任务等待、唤醒和迁移
映射为 `ipc-waitqueue` 的调度器等待队列，并保存每个线程的 robust-futex 登记状态。

## 模块分层

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 聚合层 | `src/lib.rs` | 重导出版本化 API 与 task 实现，不保存全局状态。 |
| Futex API | `futex-api/api-v0/` | 定义 FutexKey、等待结果、错误和 robust 用户态布局。 |
| Futex 实现 | `futex-impl/impl-task/` | 维护 key、waiter、WaitQueue、wake sequence 和 robust 登记。 |
| 等待适配 | `ipc-waitqueue/` | 将条件等待、唤醒、timeout 和 requeue 委托给 task scheduler。 |
| 系统调用层 | `wateros-syscall/.../sys/ipc/` | 解析 futex 操作、访问用户字、计算超时并映射 errno。 |

实现文件按职责拆分如下：

| 文件 | 内容 |
| --- | --- |
| `futex-api/api-v0/src/key.rs` | private/shared futex key 与作用域。 |
| `futex-api/api-v0/src/wait.rs` | futex 等待结果。 |
| `futex-api/api-v0/src/robust.rs` | robust-list ABI 布局和登记快照。 |
| `futex-api/api-v0/src/error.rs` | futex 领域错误。 |
| `futex-impl/impl-task/src/registry.rs` | FutexQueue、FutexRegistry 和诊断快照。 |
| `futex-impl/impl-task/src/global.rs` | wait/wake/requeue、robust 生命周期和全局 facade。 |

## 实现说明

- futex 模块不解析 Linux `futex(2)` 的裸 ABI，不直接读取或修改用户地址，也不把错误转换为
  errno；这些工作属于 syscall 层。
- private futex key 由用户虚拟地址和地址空间作用域组成；shared futex key 必须使用 MM
  解析出的稳定共享字身份，不能直接使用不同进程中的用户虚拟地址。
- 一个任务同一时刻最多登记一个 futex wait；任务正常唤醒、超时、信号中断或异常退出时都
  必须删除登记。
- futex registry 锁只保护映射、引用计数、robust 登记和诊断计数，不得跨越 WaitQueue 操作、
  用户条件闭包、调度或 IPI。
- `wake_sequence` 解决“条件复查完成但任务尚未真正睡眠”期间发生 wake 的竞态。
- `active_users` 保护锁外正在使用的 WaitQueue，避免空队列 ID 被提前释放并复用。
- 被唤醒任务的 CPU 选择、ready queue 归属和定向 IPI 均由 `wateros-task` 负责，futex 不广播
  IPI，也不自行选择 CPU。
- 当前不支持 PI futex；bitset wait/wake 只支持 `FUTEX_BITSET_MATCH_ALL`。

## 调用链路

等待流程：

```text
sys_futex
  -> 读取用户 futex 字，确认当前值符合等待条件
  -> 构造 FutexKey
  -> registry.acquire_queue(key)，active_users += 1
  -> 登记 TaskId -> FutexKey，并取得 wake_sequence
  -> 锁外再次检查用户条件
  -> WaitQueue 在 scheduler 临界区比较 wake_sequence
  -> 未发生 wake 才把当前任务改为 Blocking 并切走
  -> wake / timeout / signal 后记录结果并删除等待登记
  -> active_users -= 1，必要时回收空队列
```

两次用户条件检查避免 futex 字已经改变却仍然睡眠；第二次检查之后不再读取用户地址，而是在
scheduler 临界区比较 sequence，避免并发 wake 落入最后一个竞态窗口。

唤醒流程：

```text
wake(key, count)
  -> registry 锁内取得既有 FutexQueue，active_users += 1
  -> 释放 registry 锁
  -> wake_sequence.fetch_add(1, Release)
  -> WaitQueue::wake_one / wake_all
  -> scheduler 将任务重新投递到合适 CPU
  -> registry 锁内更新统计、active_users -= 1，并尝试回收空队列
```

迁移流程：

```text
requeue(from, to, wake_count, requeue_count)
  -> 唤醒源队列前 wake_count 个 waiter
  -> 在 scheduler 临界区把后续 waiter 迁入目标队列
  -> 同步更新等待目标和 futex waiting_tasks 归属
```

`cmp_requeue` 在迁移前通过调用方条件闭包检查 futex 用户字；不匹配返回 `Again`。
`from_key == to_key` 返回 `Invalid`，不执行无定义的自身迁移。

## FutexKey实现功能

`FutexKey` 定义在 `futex-api/api-v0/src/key.rs`。

- private key 隔离不同用户地址空间中的相同虚拟地址。
- shared key 表示可跨地址空间识别的共享字，`private_scope` 必须为 0。
- key 可作为 `BTreeMap` 主键，使同一个 futex 字只对应一个 FutexQueue。
- syscall 层负责依据 `FUTEX_PRIVATE_FLAG` 和 MM 映射关系构造正确 key。

key 的正确性决定两个进程是否会进入同一等待队列。shared futex 若错误使用用户 VA，会导致
本应互相唤醒的进程落入不同队列。

## FutexRegistry实现功能

`FutexRegistry` 定义在 `futex-impl/impl-task/src/registry.rs`，由 `global.rs` 中的模块级
`REGISTRY` 锁保护。

- `queues` 保存 `FutexKey -> FutexQueue`；每个 queue 包含 WaitQueue、wake sequence 和
  active-users 计数。
- `waiting_tasks` 保存 `TaskId -> FutexKey`，用于取消等待、requeue 和任务退出清理。
- `robust` 保存线程的 RobustListRegistration。
- 记录 wait、wake、requeue 等低频诊断计数，并生成 debug snapshot。
- 只有 `active_users == 0` 且 `WaitQueue::try_release_empty()` 成功时才能删除空队列。
- 所有取得队列句柄后可能阻塞或进入 scheduler 的操作，都必须先释放 registry 锁。

## Wait与Wake实现功能

主要全局接口位于 `futex-impl/impl-task/src/global.rs`。

- `wait_while`：按条件等待，可带 scheduler tick deadline，并区分正常唤醒、条件改变、超时和
  信号中断。
- `wake` / `wake_all`：唤醒指定 key 上的有限个或全部任务；key 不存在时返回 0，不创建空队列。
- `requeue` / `cmp_requeue`：唤醒部分 waiter，并把后续 waiter 原子迁移到另一 key。
- `cancel_task_wait`：任务异常退出或取消时删除 waiting-task 登记并释放队列使用权。
- `log_debug_snapshot`：输出 registry、queue 和 waiter 状态，只应在低频排障路径调用。

`FutexWaitOutcome` 和 `FutexError` 是 IPC 领域结果。syscall 层负责将 `Again`、`Invalid`、
`TimedOut`、`Interrupted` 等结果转换为 Linux 返回值和 errno。

## RobustFutex实现功能

robust futex 只在 IPC 层登记线程提供的链表头，不在 IPC 锁内遍历用户内存：

```text
set_robust_list
  -> robust[task_id] = { head, len, user_aspace }

get_robust_list
  -> 返回现有登记；未登记时返回默认空值

线程 exit / reap
  -> take_robust_list(task_id)
  -> syscall 层锁外遍历用户链表
  -> 写 FUTEX_OWNER_DIED 并唤醒对应 futex waiter
```

- `take_robust_list` 一次性取走登记，避免同一线程被重复清理。
- `drop_robust_list` 是幂等清理接口，可用于创建失败、回滚和 reap。
- 用户坏地址处理、PI 节点跳过和 owner-died 写入由 syscall/MM 层完成。

## Futex聚合层实现功能

`ipc-futex/src/lib.rs` 只负责导出 API 和 `impl-task`：

- 对外统一提供 `FutexKey`、`FutexError`、`FutexWaitOutcome` 和 robust 类型。
- 重导出 wait/wake/requeue、取消等待、robust 生命周期和诊断接口。
- 调用方应通过 `ipc::futex` 使用这些接口，不直接依赖 `impl-task` 或持有 FutexRegistry 锁。

排查 futex 卡顿时，重点对照 debug snapshot 与 task 的 `Blocking(WaitQueue(...))` 状态：检查
wake 调用是否推进、active-users 是否长期不归零、任务是否已经被 scheduler 唤醒却没有完成
wait 收尾，以及 shared key 是否使用了稳定共享身份。

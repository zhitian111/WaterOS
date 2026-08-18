# futex API v0 离线开发手册

本文说明 futex 的稳定数据契约。队列实现见
[impl-task](../../futex-impl/impl-task/README.md)，完整模块边界见
[ipc-futex](../../readme.md)，syscall 接入见
[sys/ipc](../../../../wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/README.md)。

## 1. 三层职责

```text
syscall 层
  参数/命令解码、用户内存访问、MM key 派生、errno 映射、robust 链遍历
      |
futex API v0
  FutexKey、错误/结果、等待结果、robust ABI 布局
      |
impl-task
  队列 registry、waiter 元数据、lost-wake 序列、WaitQueue 调度操作
```

API crate 不读取用户 futex 字，不遍历用户指针，也不直接阻塞任务。新增命令时先
判断逻辑属于哪一层，避免让 IPC registry 在持锁时访问用户内存。

## 2. `FutexKey`：最重要的不变量

```rust
pub struct FutexKey {
    pub uaddr: usize,
    pub is_private: bool,
    pub private_scope: usize,
}
```

- private futex：`uaddr` 是原始用户 VA，`private_scope` 是地址空间身份；同一 VA 在
  两个进程中必须得到不同 key。
- shared futex：`uaddr` 字段保存 MM 解析出的稳定共享字身份，`private_scope` 固定为
  0；两个进程把同一共享页映射到不同 VA 时必须得到相同 key。

`FUTEX_PRIVATE_FLAG` 为 `128`。`FutexKey::from_syscall` 只能方便测试 private flag；
它在 shared 分支直接使用传入地址，生产 syscall 绝不能把用户 VA 原样传进去。正确
路径是：

```text
解析 futex_op
  -> private: FutexKey::private(uaddr, current_user_aspace)
  -> shared: MM 查询映射及页内偏移
             -> FutexMappingIdentity
             -> FutexKey::shared(stable_identity)
```

若 shared key 派生错误，典型现象是父子进程各自在不同队列睡眠，wake 返回 0；或者
物理页复用后错误唤醒无关进程。稳定身份必须同时避免 VA 别名和物理页复用 ABA。

## 3. 错误与等待结果

`FutexError` 由 syscall 层最终映射为 Linux errno：

| IPC 错误 | syscall errno | 含义 |
| --- | --- | --- |
| `Again` | `EAGAIN` | 用户字与期望值不符 |
| `Fault` | `EFAULT` | 用户内存不可访问 |
| `Invalid` | `EINVAL` | 地址、bitset、timeout 或组合非法 |
| `Nosys` | `ENOSYS` | 命令/变体未实现 |
| `TimedOut` | `ETIMEDOUT` | 等待超时 |
| `Interrupted` | `EINTR` | 被信号中断 |

`FutexWaitOutcome` 是 impl 等待状态，不等同于错误：

- `Woken`：正常 wake；不保证用户字已变成调用者想要的值；
- `ConditionChanged`：阻塞前复查发现条件不再成立；通常映射 `EAGAIN`；
- `TimedOut`：deadline 先到且条件仍成立；
- `Interrupted`：信号或异步事件中断。

task 层只有 `Woken/TimedOut/Interrupted`。`ConditionChanged` 是 futex 在访问用户字的
预检/复检阶段产生的。

## 4. wait 的并发调用链

当前实现的关键流程是：

```text
sys_futex(FUTEX_WAIT...)
  -> 校验地址、值、bitset、timeout
  -> MM 派生 FutexKey
  -> impl::wait_while(task_id, key, bitset, timeout, condition)
     -> 条件预检
     -> registry 锁内 acquire_queue + 登记 waiter
        + 读取队列和 waiter wake_sequence 基线
     -> registry 锁外再次检查用户条件
     -> WaitQueue 的 scheduler 临界区只比较原子 sequence
     -> 阻塞 / 立即返回 / 超时 / 中断
     -> registry 锁内 finish_waiting_task + release_queue
  -> outcome 映射为 syscall 返回值
```

为什么需要 `wake_sequence`：waker 可能恰好运行在“用户字复查完成”与“任务真正加入
scheduler 队列”之间。waker 先以 Release 增加 sequence，waiter 在调度临界区以
Acquire 比较；序号变化会阻止它继续睡眠。bitset waiter 另有每任务 sequence，避免
一次选择性 wake 错误放行不匹配 waiter。

registry 锁只保护元数据。访问用户内存和 wait/wake/requeue 都必须在释放 registry
锁后完成，否则缺页、调度或锁重入会把全局 futex 服务锁死。

## 5. wake 与 requeue

普通 wake 的顺序是：取得现存队列并增加 `active_users`，释放 registry 锁，先发布
sequence，再调用 waitqueue wake，最后回到 registry 记录并释放使用权。没有队列时
直接返回 0。

`wake_bitset` 只选择 waiter bitset 与 wake mask 有交集的任务。mask 为 0 是非法输入，
不能解释为“唤醒全部”。

requeue 同时持有源和目标的 registry 使用权，但 scheduler 操作仍在 registry 锁外。
源与目标 key 相同必须返回 `Invalid`。`cmp_requeue` 的条件在 scheduler 临界区执行，
只能做不会阻塞、不会重入 futex 的短检查；条件不满足返回 `Again`。scheduler 迁移后，
registry 中每个 waiter 的 key 也必须同步更新。

## 6. 队列生命周期

registry 为每个 `FutexKey` 保存队列、原子 wake sequence 和使用计数，并另存 task 到
waiter 的反向关系。`active_users` 不仅计算已经睡眠的任务，还覆盖已经拿到队列、但
暂时在 registry 锁外执行 wake/requeue 的 CPU。

只有 `active_users == 0` 且 scheduler 队列为空时，才能从 map 删除并尝试回收
`WaitQueueId`。退出、信号或异常回滚必须调用幂等的 `cancel_task_wait(task_id)`；只从
scheduler 移除任务但不清反向表，会永久保留队列并最终耗尽内核堆。

定位 fork-heavy 内存增长时，应同时观察：futex key 数、waiter 反向表数、
`active_users`、空队列删除次数和任务退出取消次数，而不能只看用户态 `/proc/meminfo`。

## 7. robust futex ABI

64 位 ABI 类型为：

```rust
#[repr(C)]
pub struct RobustListHead {
    pub list: usize,
    pub futex_offset: isize,
    pub list_op_pending: usize,
}
```

`ROBUST_LIST_HEAD_SIZE` 在当前 64 位目标上必须为 24；链表节点指针字段大小由
`ROBUST_LIST_ENTRY_SIZE` 给出。常量：

- `FUTEX_OWNER_DIED = 0x4000_0000`；
- `FUTEX_WAITERS = 0x8000_0000`；
- `FUTEX_TID_MASK = 0x3fff_ffff`；
- `ROBUST_LIST_LIMIT = 4096`，用于限制恶意或损坏链表的遍历。

`RobustListRegistration { head, len, user_aspace }` 只登记用户地址。线程退出路径一次性
`take` 登记，再由 syscall 层在原地址空间中安全遍历用户链，给相应 futex 字设置
`OWNER_DIED` 并唤醒 waiter。IPC impl 不能直接解引用这些地址。

必须同时处理普通链节点和 `list_op_pending`，每次用户读取都可能 `EFAULT`。坏链、环、
极端偏移不得导致内核无限循环或地址算术溢出。

## 8. 扩展 syscall 实例：新增 WAIT 变体

若新增一个带 bitset 和绝对时间的 WAIT 变体，按以下顺序实现：

1. syscall 层解析 command 与 flags，拒绝未知位；
2. 校验 futex 地址对齐、bitset 非零和 timespec 合法；
3. 区分 realtime/monotonic 与 absolute/relative，安全换算为 scheduler deadline/ticks；
4. 通过 MM 派生 private 或 shared `FutexKey`；
5. 在 syscall 层构造可重复调用的用户字读取闭包；读取失败必须保留 `EFAULT`，不能
   偷换成“条件变化”；
6. 调用 `wait_while`，不在 API 或 registry 中复制用户访问代码；
7. 将四种 outcome 分别映射，尤其区分 `ConditionChanged -> EAGAIN` 与
   `TimedOut -> ETIMEDOUT`；
8. 任务退出/取消路径接入 `cancel_task_wait`；
9. 增加同地址空间 private、跨地址空间 shared、wake-before-sleep、零超时、信号中断、
   非匹配 bitset 和地址空间销毁测试。

若新增 PI futex，不能简单复用当前非 PI registry：PI 需要所有权、优先级继承、死锁
检测和 owner-exit 协议。当前接口没有承诺 PI 语义，未实现命令应明确返回 `ENOSYS`。

## 9. 常见故障定位

- **wake 返回 0 但应有跨进程 waiter**：首先检查 shared key 是否来自 MM 身份。
- **低概率永久挂起**：检查 waiter 发布、sequence 基线和 wake 发布顺序。
- **仅 bitset 压测异常**：检查每 waiter sequence 与 registry bitset 元数据是否同步。
- **地址空间销毁后写回失败**：检查 robust 清理由谁持有原 `user_aspace`，是否在 MM
  已销毁后才访问用户链。
- **堆占用单调增长**：检查所有 wait 返回、信号、exec/exit 和异常路径是否注销 waiter
  并释放 `active_users`。
- **全系统 futex 停顿**：检查 registry 锁内是否发生用户访问、调度或 futex 重入。
- **错误 errno**：区分值不匹配、用户访问失败、超时和信号中断，不能统一返回 EAGAIN。

## 10. 修改后的验证清单

- private 同 VA、不同地址空间不会碰撞；
- shared 同一页不同 VA 会汇合，不同页不会碰撞；
- wake 正好落在登记—入队窗口时 waiter 不会睡死；
- wake-one、wake-all、bitset 和 requeue 返回数量正确；
- timeout、signal、task exit 后 registry 无残留；
- robust 头大小为 24，坏链最多遍历 4096 步；
- 地址错误只返回 `EFAULT`，不会触发内核 panic；
- SMP 压测下 key 数和内核堆使用能回落；
- 执行 API 单元测试及 `make check ARCH=rv PROFILE=pre`、
  `make check ARCH=la PROFILE=pre`。


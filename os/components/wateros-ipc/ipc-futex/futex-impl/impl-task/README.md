# Task Futex 实现手册

[IPC 总览](../../../README.md) · [等待队列](../../../ipc-waitqueue/waitqueue-impl/impl-task/README.md)

该实现把 `FutexKey` 映射到 task wait queue，维护 per-task waiter bitset/sequence、queue使用权和 robust登记。它不构造 private/shared key、不读取普通 futex用户字、不遍历 robust用户链，也不决定 Linux errno；这些在 syscall+MM层。

## Registry 不变量

`FutexRegistry` 由一个全局 spin Mutex保护：

- `queues[key] = { WaitQueue, Arc<AtomicU64> global_sequence, active_users }`；
- `waiting_tasks[task] = { key, bitset, Arc<AtomicU64> waiter_sequence }`；
- `robust[task] = { head,len,user_aspace }`；
- 若干饱和统计与 last-operation诊断。

每次 `acquire_queue/acquire_existing_queue` 最终必须有一次 release。`active_users` 包含每个 sleeping/注册中 waiter的一份引用，以及每个锁外 wake/requeue操作的一份引用。只有它为零且 scheduler queue empty 时，才能 `try_release_empty` 并删除 key，防止旧 WaitQueueId 被复用后误唤醒新对象。

acquire/迁移用 `saturating_add`，release用 `saturating_sub`；这会掩盖理论溢出或不平衡释放。生命周期计数应使用 checked arithmetic并在 debug硬失败，否则错误可能提前复用ID或永久泄漏queue。

## WAIT 精确时序

```text
锁外 condition 预检
 -> registry acquire queue + publish waiting task/bitset
 -> 在同一锁内采样 global 与 per-waiter sequence
 -> 解锁，再次 condition 复检
 -> scheduler 临界区只比较两个原子 sequence
 -> Blocking / 发现已 wake
 -> 返回后 registry finish + release
```

这样避免 wake发生在“第二次用户值读取—scheduler入队”之间的永久睡眠。condition可 fault，所以只能在 scheduler锁外执行；scheduler闭包只读原子。返回 Woken也不保证用户值已变，libc必须循环。

timeout=None走无限等待；Some(0)会再读 condition后直接 TimedOut/Changed；有限 ticks映射 Woken/TimedOut/Interrupted。timeout单位为 task tick，timespec换算在syscall层。

同一 TaskId新登记会替换旧 waiting_tasks并 release旧 key。正常调度不应让一个 task并发执行两个 wait；异常退出必须调用幂等 `cancel_task_wait`，否则active引用和queue永久残留。

## 普通 WAKE 的已知过唤醒竞态

`wake(key,n)` 当前先对共享 `global_sequence.fetch_add(1)`，再 scheduler `wake_one` 最多 n次。所有已经登记但尚未进入 scheduler队列的 waiter都会观察到全局 sequence变化，于是不再睡并返回 Woken，即使它们不在 n个 wake_one选择中。

因此在该窄窗口，`FUTEX_WAKE(1)` 可能让多个 waiter返回，而函数返回值只统计真正 `wake_one` 的数量。spurious wake通常因用户循环仍保持功能正确，但 wake count、惊群和严格兼容测试不准确；forkheavy可放大调度负担。

`wake_bitset` 已采用 per-waiter sequence：锁内选最多 n个匹配 TaskId，逐个推进私有序列并定向 wake；即使尚未入队也只选中这些 waiter。普通 wake应复用“match all”的定向选择，而不是推进全局 generation。返回值应计选择的 waiter，并处理快照后task退出。

## BITSET 和复杂度

bitset=0或max=0直接返回0。匹配条件是 `waiting.bitset & wake_bitset != 0`。快照后 `wake_task` 返回false也计入selected，因为该 waiter可能正处于登记—入队窗口，私有sequence已保证它不会继续睡。

`matching_waiters` 在整个 `waiting_tasks: BTreeMap<TaskId,...>` 上过滤，并非key局部索引，复杂度O(所有 futex waiter)。大量无关key时 bitset wake延迟增长。优化可增加锁内一致的 `(key,task)` 索引，但 task scheduler仍是Blocking状态唯一真相。

## REQUEUE 的同类竞态

requeue取得source/target各一份active引用，解锁后做scheduler detailed迁移，再锁内只按实际 moved TaskId把waiting_tasks key与active引用迁移。source==target明确Invalid；source缺失时仍执行condition，匹配返回0、不匹配Again。

condition成功时当前推进source全局sequence。所有尚未入队的source waiter会返回Woken，而只有scheduler实际moved列表被迁到target；wake/requeue数量可能超过请求。修复需把“已登记未入队”纳入原子选择/迁移，或给每个waiter统一状态机和私有sequence。

CMP_REQUEUE先在scheduler锁外预读用户字，再在 `requeue_to_detailed_while` 的scheduler临界condition中复读，取得比较/迁移线性化点。第二次 `read_user_u32_in_aspace` 仍可能遇到并发unmap/mprotect；预读不能保证不fault。禁止在scheduler自旋临界做可阻塞fault。应使用pin/no-fault原子读取，或MM提供已验证的nonfaulting access，失败只快速返回。

## 锁、分配与诊断

registry锁内不得 scheduler wait/wake、普通用户访问或重入futex。源码先取 WaitQueue/Arc快照后解锁操作，再重锁记账。跨source/target无需同时拿两个业务bucket锁，因为这里只有单registry锁。

`matching_waiters`、debug snapshot、BTreeMap/Arc创建会用不可失败heap分配；kernel heap OOM会panic。大量短命key产生BTreeMap churn，应观测 `log_debug_snapshot` 的queue数/active_users、wait/wake计数和heap。日志快照先clone诊断再解锁输出，避免持registry锁进入console。

## Robust 生命周期

`set_robust_list`只验证 `len == ROBUST_LIST_HEAD_SIZE`，保存head和当时aspace handle，不验证链内容。get未登记返回 `(0, ABI size)`。exit必须先 `take_robust_list` 保证一次性，然后锁外限长遍历、处理pending、OWNER_DIED和wake；坏地址/环只能终止本线程清理，不能持IPC锁。

exec、thread exit、group exit和异常kill都要覆盖robust与普通waiting task清理。`drop_robust_list`只删登记，不执行OWNER_DIED，不能用于应通知等待者的正常owner退出。

## 扩展实例：PI futex

不能把 FUTEX_LOCK_PI映射到普通WaitQueue后返回成功。PI需要owner TID、按有效优先级排序、捐赠链/环检测、owner死亡、requeue_pi和scheduler优先级恢复。未完整实现应ENOSYS/EOPNOTSUPP，避免用户误认为实时互斥成立。

## 回归清单

- private/shared key、unaligned/bad用户地址、值预检/复检变化；
- timeout 0/有限、interrupt、spurious wake、task异常退出；
- 人工停在“registry登记后、scheduler入队前”，验证 WAKE(1)不超过选择数；
- bitset交集/零bitset/max=0、选择后task退出；
- wake count、queue/per-waiter sequence wrap；
- requeue wake N/move M、CMP不匹配、source缺失、同key；
- cmp第二次读取与munmap/mprotect竞态不在scheduler锁内fault；
- active_users acquire/release/migrate平衡，queue ID安全复用；
- robust set/get/take、坏链/环/超长、exec/exit/kill；
- 数十万短命key、bitset O(N)延迟、BTreeMap/Arc heap回落；
- `forkheavy` 下无惊群、stale waiter、永久queue或kernel heap线性增长。

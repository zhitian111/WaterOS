# 性能优化：IPC 与同步原语（futex / pipe / signal / waitqueue / shm / poll·epoll）

## 用途

汇总 `wateros-ipc` 与 poll/select/epoll syscall 的性能瓶颈、惊群、持锁跨调度与资源回收隐患。重点：futex requeue 持全局锁唤醒、exit 不回收空 futex 队列、poll/select O(nfds) 轮询与 epoll 缺失、pipe 惊群、signal 投递查表。

## 事实来源

- 代码静态链路分析；测例缺口 `os/ltp_log/todo/epoll_poll.md`、`fcntl_file_lock.md`。
- 关联子链路分析见 [ipc-subagent](0977065a-2981-472f-97fd-053c931ade50)。
- 交叉参考：`docs/audits/resources/ipc-shm-futex-signal.md`、`docs/audits/locks/ipc-futex-signal-shm.md`、`docs/audits/locks/ipc-pipe.md`、`docs/audits/resource-inventory.md`（futex 队列/unix 队列无上限）。

## 覆盖范围

`os/components/wateros-ipc/{ipc-futex,ipc-pipe,ipc-signal,ipc-waitqueue,ipc-shm,ipc-event}`、`os/components/wateros-syscall/syscall-impl/impl-kernel/src/{poll_engine.rs,sys/futex.rs,sys/signal.rs,sys/kill.rs,sys/robust.rs,sys/shm.rs}`。

> 注：与 `perf-hotpath.md` H-6/H-7 在 signal pending 快路径与 wait queue 索引上协同。

---

## 优化点清单（按预期收益从高到低）

### I-1. poll/select 全量 O(nfds) 扫描 + 无 epoll 就绪列表（事件驱动缺失）【高】

- **位置**：`os/components/wateros-syscall/syscall-impl/impl-kernel/src/poll_engine.rs:233-273,316-346,466-536,599-654`、`syscall-impl/impl-kernel/src/lib.rs:687-719`、`os/components/wateros-ipc/ipc-event/src/lib.rs:1-18`
- **当前实现/复杂度**：`scan_pollfds` 每 pollfd 调 `poll_revents_fd`，O(nfds×探测)；阻塞循环 `still_waiting` 每轮再 `scan_count()` 又 O(nfds)；select/pselect `for fd in 0..nfds` 逐 fd，O(nfds)；epoll 四 syscall（20/21/22/281）→ ENOSYS，`ipc-event` 仅占位 crate。
- **问题**：无 interest 注册表、无 per-fd 就绪回调；nfds 大或高频 poll 时 CPU 与 fd 表锁竞争线性放大；epoll 完全不可用（LTP ~33 TFAIL）。
- **改进方案**：实现 `EpollInstance` + interest 列表，`epoll_wait` 仅扫已注册 fd，阻塞按 fd 类型挂 WaitQueue，I/O 就绪 O(1) 定向唤醒；后续 `EPOLLET`/`EPOLLONESHOT`/`EPOLLRDHUP`。poll/select 短期：维护本次 monitored fd 列表避免每 tick 扫满 nfds，合并 `wait_ticks` 为整段 deadline。
- **预期收益**：高，I/O 密集与 epoll 测例 ROI 最大。
- **架构差异**：无。
- **风险/依赖**：需 VFS `EpollHandle`、fd 生命周期与 poll_engine 语义对齐；详见 `epoll_poll.md` 阶段 A–C。

### I-2. FUTEX_REQUEUE 在 FutexHub 全局锁内调用调度器唤醒 【高】

- **位置**：`os/components/wateros-ipc/ipc-futex/futex-impl/impl-task/src/hub.rs:92-105`、`os/components/wateros-task/task-scheduler/scheduler-api/api-v0/src/wait_queues.rs:459-515`
- **当前实现/复杂度**：`requeue()` 在 `with_tables`（`spin::Mutex<FutexTables>`）临界区内调 `from_wq.requeue_to(...)`，循环 `wake_one_in_wait_queue` + `pop_front` 迁移，O(wake+requeue)，全程持 FutexHub 锁 + 调度器 InterruptGuard。对比 `wake()`/`wake_all()` 已在锁外操作（`hub.rs:117-138`），requeue 是特例。
- **问题**：pthread condvar / `FUTEX_REQUEUE` 热点长时间占全局 FutexHub 锁，阻塞并发 `get_queue`；持锁跨调度延长临界区。
- **改进方案**：锁内仅解析/创建 `WaitQueueId` 并快照待迁移列表，释锁后调 `requeue_wait_queue`/批量 wake，最后短锁 `cleanup_empty_queue`。
- **预期收益**：高，多线程同步原语核心路径。
- **架构差异**：无。
- **风险/依赖**：保证 requeue/wait 的 Mesa 语义与超时队列 handle 更新仍正确。

### I-3. 任务 exit/kill 时 futex 空队列不回收 → BTreeMap 与 WaitQueueId 泄漏 【高】

- **位置**：`os/components/wateros-task/task-scheduler/scheduler-api/api-v0/src/wait_queues.rs:112-141,197-209`、`os/components/wateros-ipc/ipc-futex/futex-impl/impl-task/src/hub.rs:42-87`
- **当前实现/复杂度**：任务在 futex 上阻塞时 exit，`detach_task_from_run_queues` 从 VecDeque 移除 waiter 但不通知 FutexHub；空队列仅由 wait/wake/requeue 正常返回路径 `cleanup_empty_queue` 回收，exit 路径永不触发 → 每个曾 wait 且异常退出的 `FutexKey` 永久占 1×BTreeMap 项 + 1×WaitQueueId 槽。`detach` 对 `wait_queues` 全 Vec 逐槽 `retain`，O(W×队列长)，W 泄漏时单调增。
- **问题**：长跑 LTP / 恶意 futex key 扫描致表无界增长；exit 路径放大调度器 detach 成本。与 `perf-hotpath.md` H-7 同根。
- **改进方案**：exit/reap 钩子，任务摘除 WaitQueue 后对对应 FutexKey `cleanup_empty_queue`（需 task→key 反向索引或 per-task 记录当前 futex key）；表项/WaitQueueId 软上限 + warn + -ENOMEM。
- **预期收益**：高，资源耗尽与 exit 路径延迟。
- **架构差异**：无。
- **风险/依赖**：与 task.rs exit 顺序、robust 清理顺序协调。

### I-4. 调度器 interrupt/wake 线性扫描全部 WaitQueue 槽位 【高（I-3 未修时）】

- **位置**：`os/components/wateros-task/task-scheduler/scheduler-api/api-v0/src/wait_queues.rs:306-378,625-629`
- **当前实现/复杂度**：`finish_blocked_task`/`interrupt_task` 依次扫 blocked/sleep/全部 wait_queues/exit/child_exit，`take_task_id_by_id` 用 `VecDeque::retain` O(n)，最坏 O(W×n)；与 I-3 泄漏叠加后 W 可达数千+。
- **问题**：signal interrupt、kill 在 futex/pipe/signal 等待上频繁调用。
- **改进方案**：TCB 记录 `current_wait_handle`，interrupt O(1) 定位；`take_task_id_by_id` 改 pop+push 或 intrusive 链表。
- **预期收益**：高（I-3 未修时）/中（表项有界后）。
- **架构差异**：无。
- **风险/依赖**：wait/requeue 迁移时维护 TCB 反向指针。

### I-5. Pipe 读写成功路径 `wake_all` 惊群 【中】

- **位置**：`os/components/wateros-ipc/ipc-pipe/pipe-impl/impl-ringbuf/src/kernel_pipe.rs:161-183,275-360`
- **当前实现/复杂度**：`try_read` 成功 `write_wait.wake_all()`，`try_write` 成功 `read_wait.wake_all()`；dup/fork 多 fd 共享 pipe 时唤醒全部阻塞者，Mesa 语义下仅一个成功，其余重睡，O(等待者数) churn。属修复 P-2（wake_one 饿死）的代价。
- **问题**：多读者/多写者 + poll 多 fd 同 pipe 时 CPU 空转。
- **改进方案**：单消费者/单生产者用 `wake_one`，多 ref 时按所需字节 `wake min(waiters, needed)`；poll 多 fd 改 level-triggered 就绪位 + 单 wait 队列。
- **预期收益**：中，多 fd 同 pipe / shell 管道以外场景。
- **架构差异**：无。
- **风险/依赖**：避免回归 dup 多读者饿死（P-2）。

### I-6. poll 阻塞：1-tick 切片 + 串行 per-fd wait + 重复全表扫描 【中】

- **位置**：`os/components/wateros-syscall/syscall-impl/impl-kernel/src/poll_engine.rs:275-344,560-624`
- **当前实现/复杂度**：`wait_ticks = remaining.min(1)`，每 pipe fd 最多睡 1 tick；`poll_wait_pipe_fds` `for i in 0..nfds` 串行 `poll_wait_for_ticks`；`still_waiting` 每次 `scan_count()` 又 O(nfds)。
- **问题**：nfds 大 + 短 timeout 时外层 loop 次数 ≈ timeout_ticks×nfds，latency 与 CPU 浪费。
- **改进方案**：单 deadline 合并 wait；多 fd 用 poll 专用 WaitQueue 统一 sleep 至任一 fd 就绪；降低 scan_count 频率。
- **预期收益**：中，多 fd poll/ppoll。
- **架构差异**：无。
- **风险/依赖**：保留 `with_current_io` 借出 fd 时不可在 condition 内重扫同 fd 的约束（避免 POLLNVAL 忙等）。

### I-7. SignalRegistry `send_process` / kill 全表线程扫描 + 重复持锁 【中】

- **位置**：`os/components/wateros-ipc/ipc-signal/src/lib.rs:434-466`、`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/kill.rs:75-88`
- **当前实现/复杂度**：`send_process` 两次 `threads.iter().filter(pid==...)`，第二次对每候选 `has_deliverable`，O(T)；kill Pending 对进程每 member 单独 `with_registry` + `interrupt_task`，O(M×锁)；`drop_thread_and_empty_process` `threads.values().any` O(T)。
- **问题**：线程数增长后信号投递与 kill 延迟线性上升；kill 与 send_process interrupt 范围不一致。
- **改进方案**：`BTreeMap<pid, Vec<task_id>>` 进程内线程索引；send_process 一次扫描选 target；kill 仅 interrupt `dispatch.target_task_id`。
- **预期收益**：中，多线程进程 + 高频 kill/tgkill/timer。
- **架构差异**：无。
- **风险/依赖**：fork/exit 维护索引；Linux 语义对齐测试。

### I-8. 每次返回用户态查 pending 信号抢全局 registry 锁 【中】

- **位置**：`os/src/trap_handler.rs:283-284,314-338`、`os/components/wateros-ipc/ipc-signal/src/lib.rs`（`take_deliverable`）
- **当前实现/复杂度**：几乎所有 syscall 成功/失败、部分 fault 路径 `return_to_user_signal_delivery` → `take_deliverable` → `SIGNAL_REGISTRY.lock()`；`take_deliverable` 本身 O(1)（u64 `first_signal` 位运算）但每次 syscall 一次全局 Mutex。
- **问题**：高频 syscall（getpid/read/write 循环）无谓抢锁；无 per-thread 快路径。
- **改进方案**：TCB 缓存 `deliverable_bits`（thread.pending ∪ process.pending − mask），send_* 时更新，trap 返回先读 TCB 无锁快路径，有 pending 再进 registry。与 `perf-hotpath.md` H-6 同一改造。
- **预期收益**：中，syscall 密集 benchmark。
- **架构差异**：无。
- **风险/依赖**：mask/sigsuspend/ppoll 临时 mask 一致性。

### I-9. `real_deadlines` 陈旧索引未清理致 `expire_realtime` 冗余 【中】

- **位置**：`os/components/wateros-ipc/ipc-signal/src/lib.rs:241-246,569-591,634-680`
- **当前实现/复杂度**：`set_timer(ITIMER_REAL)` 替换 timer 只插新 deadline 不删旧；`drop_process` 不清该 pid 条目；`expire_realtime` `range(..=now)` 收集 + 逐 entry generation 过滤 stale，每 tick O(D+E) 含大量无效 entry。
- **问题**：长跑后 BTreeMap 膨胀；timer 频繁 set 时 CPU 浪费。
- **改进方案**：set_timer 删旧 `(deadline,pid,gen)`；drop_process sweep；或 per-pid 单条 deadline 指针 O(1) 过期。
- **预期收益**：中，长运行 + 频繁 setitimer。
- **架构差异**：无。
- **风险/依赖**：generation 逻辑已存在，扩展删除即可。

### I-10. SysV SHM `shmget` 持全局锁逐页分配 + 多次 `pages.clone()` + fork 失败不回滚 【中（性能）/ 高（fork 回收正确性）】

- **位置**：`os/components/wateros-ipc/ipc-shm/src/lib.rs:101-131,157-166,290-346`、`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/shm.rs:13-117`、`sys/clone.rs:310-313`
- **当前实现/复杂度**：`create_or_get` 在 ShmRegistry 锁内 `alloc_segment_pages`，最多 4MiB/4KiB=1024 次 `frame_alloc`+zero；`begin_attach`/`detach`/`fork_task` 均 `segment.pages.clone()` O(页数)；fork MM 映射失败仅 warn 不回滚。
- **问题**：shmget 阻塞无关 shm/futex 路径；大段 attach/fork 重复克隆 PPN 列表；fork MM 失败致 nattch/帧账本漂移。
- **改进方案**：锁外分配帧，锁内仅插元数据；`Arc<[PhysPageNum]>` 共享页列表；fork 两阶段 commit/rollback；全局限额（段数/总页数）。
- **预期收益**：中（性能）+ 高（fork 失败回收，属正确性）。
- **架构差异**：无。
- **风险/依赖**：与 Sv39/loongarch MM 共享映射 API 一致。

### I-11. Robust list 退出清理：4096 步上限 + 每节点用户拷贝与双 key wake 【中】

- **位置**：`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/robust.rs:46-112`、`os/components/wateros-ipc/ipc-futex/futex-api/api-v0/src/robust.rs:10`
- **当前实现/复杂度**：线程 exit `while steps < 4096` 遍历用户链表，每节点 `read_user_u32` + 可能双 key `wake_all`；`drop_robust_state`（reap）不遍历 wake；O(min(链长,4096)×wake)；超长链静默截断。
- **问题**：exit 路径延迟与 futex 表压力；截断遗留 OWNER_DIED 未设 → 永久 wait；reap 语义缺口。
- **改进方案**：超限 warn + 计数；批量 wake 合并 key；内核缓存 robust 节点避免 exit 读用户链；reap/exit 统一清理。
- **预期收益**：中，pthread robust mutex 重负载 exit。
- **架构差异**：无。
- **风险/依赖**：用户链表不可信，须保留步数上限。

### I-12. Futex 表 `BTreeMap<FutexKey>` + wake 循环多次进调度器 + WAIT 侧无 alternate key 【中（性能）/ 高（永久睡眠正确性）】

- **位置**：`os/components/wateros-ipc/ipc-futex/futex-impl/impl-task/src/hub.rs:14-40,117-131`、`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/futex.rs:86-96,141`
- **当前实现/复杂度**：队列键 BTreeMap O(log K)；`wake(max_wake)` `for _ in 0..limit { wq.wake_one() }`，每次独立 InterruptGuard；WAIT 侧不尝试 alternate private/shared key（仅 wake 侧 `wake_with_alternate_keys`）→ 部分路径永久睡眠、队列滞留。
- **问题**：高 wake 批量时调度器进出次数多；key 不一致致队列只增不减。
- **改进方案**：HashMap + 固定 seed；批量 wake 单次调度批处理；WAIT 侧 symmetric alternate key。
- **预期收益**：中（wake 批量）/ 高（永久睡眠，正确性）。
- **架构差异**：无。
- **风险/依赖**：no_std hash crate 选择；private 语义对齐 Linux。

### I-13. Pipe 环形缓冲逐字节拷贝 【低中】

- **位置**：`os/components/wateros-ipc/ipc-pipe/pipe-impl/impl-ringbuf/src/kernel_pipe.rs:64-86`
- **当前实现/复杂度**：`read_into`/`write_from` 逐字节循环 + `% capacity`，O(count) 但常数差、无 bulk `copy_from_slice`，默认 4096B。
- **问题**：大 buffer 或 splice 类扩展时 CPU 带宽浪费；`% capacity` 阻碍向量化。
- **改进方案**：按 contiguous run 分两段 `copy_from_slice`。
- **预期收益**：低中，大 pipe / 高吞吐管道。
- **架构差异**：无。

### I-14. sigsuspend / rt_sigtimedwait 等待条件内反复抢 SignalRegistry 锁 【低中】

- **位置**：`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/signal.rs:406-413`、`sys/task.rs:530-544`
- **当前实现/复杂度**：`wait_current_while` condition 闭包内 `with_registry(has_deliverable/pending)`，每次入睡前抢全局 Mutex（`rt_sigsuspend` WaitQueue 泄漏已修，`signal.rs:413` `try_release_empty`）。
- **问题**：条件检查 + 锁竞争。
- **改进方案**：condition 读 TCB cached pending；registry 锁仅用于 begin/end_sigsuspend。与 I-8 协同。
- **预期收益**：低中。
- **架构差异**：无。

### I-15. `try_release_wait_queue` 中 `free_wait_queues.contains` 线性查重 【低】

- **位置**：`os/components/wateros-task/task-scheduler/scheduler-api/api-v0/src/wait_queues.rs:81-96`
- **当前实现/复杂度**：释放前 `free_wait_queues.contains(id)` O(F)。
- **问题**：极端泄漏场景释放也变慢；次要。
- **改进方案**：位图标记 slot 状态或保证 free 列表不重复入队。
- **预期收益**：低。
- **架构差异**：无。

### I-16. Pipe `spin::Mutex` + UP 抢占理论风险 【低】

- **位置**：`os/components/wateros-ipc/ipc-pipe/pipe-impl/impl-ringbuf/src/kernel_pipe.rs:91,279-337`
- **当前实现/复杂度**：临界区 ≤4096B ring 操作极短；持锁被 timer 抢占 → 等锁任务 UP 上自旋。
- **问题**：理论卡死/延迟，实践 LTP 未见。
- **改进方案**：临界区关抢占或换 sleeping mutex（SMP 前评估）。
- **预期收益**：低。
- **架构差异**：无。

### I-17. epoll 未实现（功能缺口汇总）【高（功能+间接性能）】

- **位置**：`os/components/wateros-syscall/syscall-impl/impl-kernel/src/lib.rs:687-719`、`os/ltp_log/todo/epoll_poll.md`
- **当前实现**：nr 20/21/22/281 → ENOSYS。
- **问题**：能力缺失，用户态只能 fall back poll/select O(n)。
- **改进方案**：见 I-1 与 `epoll_poll.md` 阶段 A–C。
- **预期收益**：高（功能 + 间接性能）。
- **架构差异**：无。
- **风险/依赖**：ABI 表 `syscall_number.rs` 双架构接线。

---

## 已修复/降级项（分析时核对，避免误判）

| 项 | 状态 | 证据 |
|----|------|------|
| futex wait 持锁跨睡眠 | 已分离 | `hub.rs:61-86` 先 get_queue 释锁再 wait |
| futex wake/wake_all 持锁跨调度 | wake/wake_all 已释锁，requeue 仍持锁 | `hub.rs:117-138` vs `92-105` |
| pipe wake_one 饿死 | 已改 wake_all | `kernel_pipe.rs:288-289` |
| sigsuspend WaitQueue 泄漏 | 已 try_release_empty | `signal.rs:413` |
| shmat TOCTOU | 已两阶段 attach | `ipc-shm/src/lib.rs:157-207` |
| robust wake 仅 private | 已双 key | `robust.rs:89-98` |

## 关键调用链速查

```
futex wait:    sys_futex → FutexHub::wait_while → [lock] get_queue [unlock] → wait_current_while → [lock] cleanup_empty_queue
futex requeue: sys_futex → FutexHub::requeue [全程 FutexHub lock] → requeue_to → scheduler
pipe read:     try_read [Mutex] → write_wait.wake_all → 全部写者就绪
poll block:    scan_pollfds O(nfds) → poll_wait_pipe_fds 串行 1-tick → still_waiting 内再 scan
signal 返回:   trap sret 前 → deliver_pending_signal → take_deliverable [registry lock]
shm attach:    [lock] begin_attach → [unlock] MM map → [lock] finish_attach
```

## 落地优先级建议

1. epoll + poll 事件驱动 / 减少 O(nfds)（I-1, I-17）
2. futex requeue 释锁唤醒 + exit 空队列回收 + interrupt O(1)（I-2, I-3, I-4）
3. SHM fork 事务回滚 + 锁外 alloc（I-10）
4. signal 进程线程索引 + TCB pending 快路径 + real_deadlines 清理（I-7, I-8, I-9）
5. pipe wake 策略 + ringbuf bulk 拷贝（I-5, I-13）
6. poll 1-tick/串行 wait 合并（I-6）

## 后续维护入口

- 改 futex/signal/shm：同步 `docs/audits/resources/ipc-shm-futex-signal.md`、`docs/audits/locks/ipc-futex-signal-shm.md`。
- 改 pipe：同步 `docs/audits/locks/ipc-pipe.md`。
- 实现 epoll：同步 `os/ltp_log/todo/epoll_poll.md`、`docs/exports/features/wateros-ipc.md`、ABI 号表。

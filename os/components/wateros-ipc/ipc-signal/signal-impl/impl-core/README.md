# Signal Core 实现离线开发手册

[IPC 总览](../../../README.md) · [Signal API](../../signal-api/api-v0/README.md) · [Syscall 信号实现](../../../../wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/signal.rs)

本 crate 管理 Linux 风格信号的内核状态机：进程 disposition、进程/线程 pending、mask、
临时等待 mask、备用信号栈和 timer。它只返回 `SignalDispatch`/`SignalEffect` 意图，不操作
scheduler，不写用户 signal frame，也不实现 errno/用户 ABI。

## 1. 文件与分层边界

| 文件 | 职责 |
|---|---|
| `state.rs` | 进程、线程、interval/POSIX timer 和 deadline 条目 |
| `registry.rs` | 生命周期、投递、pending 选择、mask、action、altstack |
| `timer.rs` | setitimer/POSIX timer/CPU timer 到期算法 |
| `global.rs` | 唯一全局锁和无副作用 facade |
| `lib.rs` | 再导出与 feature self-test |

边界调用关系：

```text
syscall/trap/task/time
  -> ipc::signal global facade
     -> Mutex<SignalRegistry> 内纯状态变更
     <- SignalDispatch / SignalEffect
  -> 锁外 wake/stop/continue/terminate、IPI、用户 frame copy
```

信号 frame 的 PC/SP/寄存器布局、`ucontext`、restorer 和 `rt_sigreturn` 都位于 syscall/trap
层。不要把架构寄存器类型放入本 crate。

## 2. 全局注册表与 ID

```text
static REGISTRY: spin::Mutex<SignalRegistry>
SignalRegistry
  ├─ processes: BTreeMap<pid, ProcessSignalState>
  ├─ threads: BTreeMap<wateros_task_id, ThreadSignalState>
  └─ real_deadlines: BTreeMap<deadline_ns, Vec<RealDeadlineEntry>>
```

`threads` 的 key 是 WaterOS 内部 task ID，不是 Linux TID；`ThreadSignalState.tid` 才用于
选择最低 TID 的进程信号唤醒候选。每个 thread 条目的 pid 必须存在于 processes。

所有 facade 调用获取同一不可重入自旋锁。锁内禁止 scheduler、IPI、user copy、日志、
等待、地址空间操作，以及再次调用 global facade。BTreeMap/Vec 插入会使用内核 heap，
当前不是 fallible：创建进程/线程/timer 或 deadline bucket 时仍可能触发 allocator panic。

`register_process/ensure_process` 使用 `entry.or_insert`，已有 task ID 不会被重绑到新 pid。
task ID 若可能复用，调用方必须先完成 exit 清理，否则会留下错误归属；接口目前不会报冲突。

## 3. 进程与线程状态

`ProcessSignalState` 在同一线程组共享：

- `actions: [SignalAction; 64]`：每个信号 disposition；
- `pending: SignalSet`：进程定向普通 pending；
- `real/virtual_timer/prof`：三种 interval timer；
- `posix_timers: BTreeMap<timer_id, PosixTimerState>`；
- `next_posix_timer_id`：进程内 ID 游标，限制到 `i32::MAX`；
- `user_cpu_ns/total_cpu_ns`：CPU timer 累计时钟。

`ThreadSignalState` 每线程独立：

- `pid/tid`；
- `mask` 和线程定向 `pending`；
- `temporary_restore_mask`：sigsuspend/ppoll/pselect6 共用的原 mask；
- `waiting_for`：sigwait/signalfd 类同步等待集合；
- `alternate_stack { sp,size,active_frames }`。

`SignalSet` 是单个 u64，信号号 1–64 映射到 bit 0–63，同号重复生成会合并。当前即使
32–64 也没有 Linux realtime signal 的 FIFO/多实例排队、siginfo payload 或 per-user
queue limit；增加 realtime 语义必须另建有界队列，不能继续只改位图。

## 4. 生命周期：create/fork/clone/exec/exit

| 路径 | actions | mask | pending | altstack | timers |
|---|---|---|---|---|---|
| 首次 register | 全 default | empty | empty | disabled | disabled/empty |
| fork 新进程 | 复制父进程 actions | 复制调用线程 | 不继承 | 复制调用线程 | 不继承 |
| CLONE_THREAD | 共享进程 actions | 复制父线程 | 新线程 empty | disabled | 共享进程 timer |
| exec | caught handler→default，SIG_IGN 保持 | 当前线程保持 | 当前实现保持 | 重置 | interval 保持，POSIX timer 清空 |
| thread exit | 共享状态不变 | 删除 | 线程 pending 丢弃 | 删除 | 最后线程前保留 |
| last thread/process exit | 删除整个 process 和 threads | — | 全清 | — | 全清 |

`exec_process(task_id, removed_task_ids)` 先删被 exec 淘汰的其它线程，再重置存活线程所属
进程。当前 exec 没有显式清除存活线程的 `temporary_restore_mask/waiting_for`；正常 exec
不应发生在这些 wait 临界区，但修改 exec/取消路径时必须验证不会把临时状态带入新镜像。

`drop_thread` 不级联 process；`drop_thread_and_empty_process` 会检查是否仍有同 pid 线程；
`exit_thread(...,last_thread)` 依赖调用方给出的 last_thread 正确。重复/乱序清理应保持
BTreeMap 不变量。

## 5. 生成与交付必须分离

```text
生成：kill/tkill/timer/fault
  -> validate signal + 查询当前 disposition
  -> 更新 thread 或 process pending，或产生 Stop/Continue 意图
  -> 返回 SignalDispatch { delivery, target_task_id }
  -> syscall/task 层 apply_signal_dispatch（锁外）

交付：目标线程返回用户态安全点
  -> take_deliverable(task_id)
  -> 用此刻 mask 和 disposition 决定 SignalEffect
  -> syscall/trap 层落实 effect
```

不能在 send 时固定 handler，因为信号 pending 期间 `rt_sigaction`/`sigprocmask` 可能改变。
普通 pending 选择最低信号号；若同号同时在线程和进程集合，先消费线程集合，进程位保留。

## 6. 生成阶段规则

显式 `SIG_IGN` 在生成时直接 Ignored，不进 pending。默认忽略的 SIGCHLD/SIGURG/SIGWINCH
仍进入 pending，使阻塞后的 signalfd/sigwait 能消费；最终异步交付时才跳过。

- `send_thread` 只写目标 thread pending；
- `send_process` 写 process pending，并优先选最低 TID 的未屏蔽/正在等待线程用于唤醒；
- SIGSTOP 不写 pending，直接返回 Stop 意图；
- SIGCONT 返回 Continue；若装有用户 handler，还把信号加入 pending；
- pending 投递只在目标未屏蔽或 `waiting_for` 包含信号时建议 wake；
- `target_task_id` 只是锁外优先处理对象，不代表信号归属于该线程。

同步 fault 使用 `force_thread_signal`：若信号被 mask 或 disposition 为 SIG_IGN，先恢复该
信号 default action、从 mask 移除，再加入线程 pending，并清 waiting_for。这样 SIGSEGV
等不能靠忽略/阻塞无限跳过。

## 7. 安全点选择与 effect

`take_deliverable` 计算 `(thread.pending ∪ process.pending) - mask`，取最低编号并读取当前
action：

- SIGKILL 或 default terminate → `Terminate`，清临时 mask/wait；
- default stop → 恢复 sigsuspend 前 mask，清临时状态，返回 `Stop`；
- SIGCONT 且无 handler → 同样恢复并返回 `Continue`；
- explicit ignore/default-ignore → 丢掉该 pending，递归选择下一个；
- user handler → 生成 `PendingSignal { signal,scope,action,previous_mask }`。

handler mask 是当前 delivery mask ∪ action.mask；没有 SA_NODEFER 时再加入当前信号。
SIGKILL/SIGSTOP 始终从 action.mask 移除。SA_RESETHAND 在返回 effect 前把 disposition 重置
为 default。`previous_mask` 若处于临时 mask 范围，取进入范围前的原 mask，供 signal frame
和 `rt_sigreturn` 恢复。

停止态不走普通 deliverable；task 层应调用 `take_sigkill`，它在线程 pending 优先、否则
进程 pending 中消费 SIGKILL，并清临时等待状态。

## 8. mask、sigsuspend、poll 与同步等待

`update_mask` 支持 BLOCK/UNBLOCK/SETMASK，set=None 只查询旧值。任何写入都会移除
SIGKILL/SIGSTOP。begin_sigsuspend 和 begin_poll_sigmask 都：保存旧 mask 到同一个
`temporary_restore_mask`，安装去掉不可屏蔽信号的临时 mask；嵌套调用返回 InvalidHow。

结束函数幂等：有保存值就恢复并 take，没有则成功。异步 handler/stop/continue/terminate
选择也会消费或清除临时状态，因此 syscall wait 返回路径不能无条件覆盖已由交付路径安排
的恢复语义。

`begin_signal_wait` 只登记 wait_set，实际等待由 syscall waitqueue 完成。`take_pending_record`
先查 thread pending，再查 process pending，并记录 scope；signalfd copy_to_user 失败必须
调用 `restore_pending_record` 放回原集合。若目标线程在此期间退出，restore 会失败，调用方
必须定义 fd/read 取消顺序。

## 9. sigaction 和不可变信号

`set_action` 拒绝非法编号以及 SIGKILL/SIGSTOP；同时从 action.mask 中移除这两个信号。
把 disposition 改为 SIG_IGN 时，立即从进程 pending 及该 pid 所有线程 pending 清掉同号
信号。改回 default/handler 不恢复已丢弃实例。

SA_SIGINFO、SA_RESTART、SA_ONSTACK、SA_RESTORER 的具体 frame/restart 行为不在 core；
core 只保存 flags，并处理 NODEFER/RESETHAND。增加 flag 时要明确是状态机处理还是 syscall
frame 处理，避免两边都做或都不做。

## 10. 备用信号栈

`replace_alternate_stack` 在 `active_frames != 0` 时返回 AlternateStackActive，否则原子替换
并返回旧值。`enter_signal_frame(true)` 饱和增加 active_frames；`leave...` 饱和减少。

global `leave_signal_frame(task_id, restored_mask, frame_sp)` 先读取 altstack、恢复 mask，再用
`alternate_stack.contains(frame_sp)` 判断这次 frame 是否在备用栈上并减少计数。contains
使用 `[sp,sp+size)`，加法溢出视为不包含。

饱和减法会掩盖重复 leave，当前不会报告 frame 计数下溢。trap 层必须保证每个成功
enter 与一次合法 sigreturn 配对；恶意/伪造 frame 不能直接驱动计数。

## 11. interval timer

三类 timer 的时钟：

- ITIMER_REAL：传入的 monotonic ns，到期发 SIGALRM；
- ITIMER_VIRTUAL：累计 user CPU ns，到期发 SIGVTALRM；
- ITIMER_PROF：累计 total CPU ns，到期发 SIGPROF。

`replace` 把 value=0 解释为禁用，否则 deadline=now+value，generation wrapping+1。周期 timer
若跨过多个周期，以原 deadline 为基准跳到第一个未来 deadline，避免按“当前时间+interval”
漂移；标准信号仍只保留一个 pending 位。

ITIMER_REAL 使用 `real_deadlines` 索引。重设/禁用不遍历删除旧条目，而在到期时核对
`pid+generation+deadline` 丢弃 stale entry。反复设置很远的 deadline 会让陈旧 bucket/Vec
一直占内存到对应时间；进程退出也只删 process，旧索引同样延迟回收。这是当前内存增长
风险，可通过可删除 timer handle 或周期压缩 stale entries 改进。

## 12. POSIX timer

timer ID 只在 pid 内有效，从 0 开始、wrap 到 i32::MAX 范围并寻找空槽。clock 支持
Realtime/Monotonic，signal 只校验 1–64。`set_posix_timer` 支持相对和 absolute deadline，
value=0 禁用，并在重设时 overrun=0。

`expire_posix_timers` 当前每次扫描所有进程和所有 POSIX timer。周期到期计算错过次数，
`overrun=min(expirations-1,i32::MAX)`，然后每个 timer 只生成一次普通进程信号。多个 timer
发同一标准信号会在 pending 位图中合并，但各 timer 自己保存 overrun。

timer BTreeMap 插入/扫描结果 Vec 都是不可失败的 alloc API；大规模 timer 可能导致 heap
OOM 或长时间占用全局 signal 锁。syscall 层应限制每进程 timer 数，长期方案应使用
fallible allocation 和 deadline heap/index。

## 13. syscall/trap 真实调用链

以 handler 交付为例：

```text
sys_kill/sys_tkill/sys_tgkill/timer_tick/fault
  -> ipc::signal::send_process/send_thread/force_thread_signal
  -> signal.rs::apply_signal_dispatch
     -> task wake/stop/continue（锁外）
trap/syscall 返回用户态
  -> syscall impl::deliver_pending_signal(frame,restart)
  -> ipc::signal::take_deliverable
  -> Handler: 构造架构 signal frame、处理 SA_ONSTACK/restart、enter_signal_frame
  -> 修改用户 PC/SP 进入 handler
sys_rt_sigreturn
  -> 校验/恢复用户 frame
  -> ipc::signal::leave_signal_frame(restored_mask,frame_sp)
```

`sys_rt_sigaction`/`sys_rt_sigprocmask` 对用户结构做 copy 和 ABI 转换后才调用 core；
`sys_rt_sigsuspend` 安装临时 mask、用独立 waitqueue 等待，并由安全点交付保存原 mask；
`sys_signalfd4` 在 fd read 中 take pending，copy 失败按 scope rollback；setitimer 位于
`sys/time/timer.rs`，POSIX timer 位于 `sys/time/posix_timer.rs`。

## 14. 新增 `rt_sigqueueinfo` 实例

当前位图不足以保存 siginfo，端到端扩展步骤：

1. API 定义内核内部 `QueuedSignal { signal, SigInfo, sequence }`，不要直接复用含指针的用户
   struct；
2. standard signal 仍可 coalesce，但需决定首次/最后一次 siginfo；realtime signal 用每进程
   有界 FIFO 并保留顺序；
3. thread/process 分别拥有队列，选择时保持 thread 优先及 realtime FIFO；
4. take/restore 记录完整 item 和 scope，signalfd EFAULT 不得丢 payload/顺序；
5. 限制每 UID/进程队列，fallible reserve 失败映射 EAGAIN，不能 panic；
6. fork 不继承 pending queue，exec 保留规则与现有位图一致，exit 释放全部 payload；
7. signal frame 和 signalfd 填充真实 siginfo；
8. 测试同号多次、跨线程/进程、queue full、EFAULT rollback、exec/exit 和 SMP 顺序。

## 15. 故障定位与已知限制

| 现象 | 检查点 |
|---|---|
| kill 成功但线程不醒 | dispatch target、mask、waiting_for、锁外 apply |
| handler mask 恢复错误 | temporary_restore_mask、previous_mask、rt_sigreturn |
| signalfd 偶发丢信号 | take_pending_record scope 与 EFAULT rollback |
| stopped 进程杀不掉 | stopped loop 是否调用 take_sigkill |
| timer 重复/漂移 | generation/deadline 校验、CPU accounting 是否重复 |
| signal 状态泄漏 | fork/clone 失败回滚、exec removed threads、last_thread 标志 |
| heap 随 timer 重设上涨 | stale real_deadlines bucket 和 infallible BTreeMap/Vec |
| realtime 信号次数不足 | 当前 u64 pending 必然 coalesce，尚无 RT queue |

其它限制：目标选择不是负载均衡；没有 siginfo queue；默认 core 错误分类较粗；注册表是一把
全局锁；altstack leave 下溢被饱和隐藏；POSIX timer 到期为 O(进程×timer) 扫描。

## 16. 自回归矩阵

- invalid/immutable signal、显式 ignore 清 pending、default-ignore 可由 signalfd 消费；
- blocked/unblocked、最低信号号、同号 coalesce、thread/process 同号两次消费；
- process target 最低未屏蔽 tid、sigwait 唤醒、目标同时 exit；
- SA_NODEFER/RESETHAND、同步 fault 强制 default、sigsuspend mask 恢复；
- signalfd thread/process scope、部分/零 EFAULT rollback；
- altstack 嵌套 frame、边界 SP、溢出范围、重复 sigreturn 拒绝；
- fork/clone/exec/exit/失败回滚/task ID 复用；
- interval 单次/周期/跨多周期/rearm stale/进程退出；
- POSIX relative/absolute、overrun、ID wrap/delete、同信号合并；
- SMP send 与 action/mask/exec/exit 并发，registry 外再执行调度副作用。

本 crate 不依赖 task/platform，可以独立运行 host 状态机测试。目标集成检查从 `os/` 执行：

```sh
cargo test --manifest-path components/wateros-ipc/ipc-signal/signal-impl/impl-core/Cargo.toml
python3 scripts/maintenance/check_offline_docs.py
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```

实现内 `registry.rs` 的 host 单测覆盖纯状态机；task wake、trap frame、user copy 和跨核调度
仍需在 syscall/目标架构集成测试中验证。

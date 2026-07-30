# ipc-signal

`ipc-signal` 维护 Linux 风格的进程/线程信号状态，不负责调度任务，也不读写用户态
signal frame。它采用与 `wateros-task`、scheduler 相同的三层结构：

```text
signal-api/api-v0
  稳定类型、常量和错误
          │
          ▼
signal-impl/impl-core
  registry：生命周期、mask、pending、disposition、投递
  state：仅实现层可见的进程/线程/timer 状态
  timer：setitimer、POSIX timer、CPU timer
          │
          ▼
ipc-signal
  薄聚合层，只重导出 API 和当前实现
```

## 边界

- API 层不持有全局状态，不依赖 task、syscall 或具体架构。
- impl 层只维护信号状态机。全局锁由
  [`global`](signal-impl/impl-core/src/global.rs) facade 内部管理。
- syscall 层负责 Linux ABI 转换、用户内存读写，以及把 `SignalDispatch` 应用到
  task（唤醒、停止、继续或终止）。
- trap/arch 层负责保存、构造和恢复用户态 signal frame。

调用方直接通过 `ipc::signal` 的模块级函数执行领域操作，不接触 `SignalRegistry`
或锁闭包。需要组合的生命周期和 signal-frame 操作由 facade 在一次加锁中完成。

`SignalDispatch` 只是**锁内计算出的意图**，不是已经完成的调度动作：例如它可能指出
“应唤醒 task 42”或“应停止一个进程”。syscall/task 层必须在 `send_*` 返回、释放
本模块锁之后，才实际唤醒、停止、终止任务或向远端 CPU 发 IPI。这样不会在信号注册表锁内
进入 scheduler、wait queue、VFS 或用户内存路径。

## 状态所有权

- 进程共享：disposition、process pending、interval/POSIX timer、CPU timer 计数。
- 线程私有：blocked mask、thread pending、`sigsuspend`/poll 临时 mask、
  `sigwait` 等待集合、备用信号栈状态。
- syscall/task：进程树、线程调度状态、wait queue、用户地址空间。

注册表的三个索引分别是：


| 索引             | key               | 内容                     | 用途                                   |
| ------------------ | ------------------- | -------------------------- | ---------------------------------------- |
| `processes`      | PID               | `ProcessSignalState`     | 进程共享 disposition、pending 和 timer |
| `threads`        | WaterOS task ID   | `ThreadSignalState`      | 线程 mask、pending、等待与备用栈       |
| `real_deadlines` | 单调时钟 deadline | `(PID, generation)` 列表 | 高效找到到期的`ITIMER_REAL`            |

其中 `task ID` 是内核内部任务标识，`tid` 是用于进程内信号选择顺序的线程 ID；两者不能
互换。`SignalSet` 是 64 位位集，表示同一种普通信号至多有一个 pending 位，并不保存
多次投递的排队次数。

主要生命周期规则：

- `fork`：复制 disposition、调用线程 mask 和备用信号栈；不继承 pending、interval
  timer 或 POSIX timer。
- `CLONE_THREAD`：共享进程状态，新线程继承 mask，不继承备用信号栈。
- `exec`：保留 ignored disposition、pending 和 interval timer；重置用户 handler、
  POSIX timer 与备用信号栈。
- thread exit：删除线程状态；最后一个线程退出时删除进程状态。

关键不变量：

- 每个已注册线程都必须指向已注册的 PID。
- `SIGKILL` 和 `SIGSTOP` 始终从 mask 与 `sigaction` 可修改范围中排除。
- `temporary_restore_mask` 同一时刻只能由 `sigsuspend`、`ppoll` 或 `pselect6` 中的一种
  临时等待占用；结束路径必须恢复并清空它。
- 信号 handler 的 mask 只在目标线程取到可交付信号时安装；因此 disposition/mask 的
  最终判断发生在安全点，而不是初次 `kill` 时。

## 投递链路

```text
kill/tkill/timer
  → SignalRegistry::send_process / send_thread
  → 更新 pending，返回 SignalDispatch
  → syscall 层应用 dispatch，必要时唤醒/停止/终止 task
  → trap 返回用户态前 SignalRegistry::take_deliverable
  → syscall 层构造 signal frame
  → arch 设置 PC/SP/参数寄存器进入 handler
  → trampoline 发起 rt_sigreturn
  → arch 恢复现场，registry 恢复线程 mask
```

`send_process` 会优先挑选最低 TID 的未屏蔽线程，或能接收 `sigwait` 的线程；进程级
pending 本身仍由整个进程共享。`send_thread` 则只修改目标线程的 pending。两条路径都
返回 `SignalDispatch`，由调用方在锁外执行其副作用。

## 定时器与 SMP

- `ITIMER_REAL` 以单调时间维护 deadline，并用 `generation` 过滤 `setitimer` 替换后
  留下的旧 deadline 项。
- `ITIMER_VIRTUAL` 根据用户态 CPU 时间累计，`ITIMER_PROF` 根据用户态加内核态 CPU
  时间累计；调用者应只对实际运行的进程记账。
- POSIX timer 在时钟到期时更新 `overrun`，再经普通 `send_process` 链路生成 dispatch。
- `REGISTRY` 是整个模块唯一的自旋锁，保护上述三张表及其跨表不变量。它能串行化多 CPU
  上的投递、mask 更新和生命周期操作，但不替代 scheduler 锁，也不保证目标 CPU 已经运行。
- 调用全局 facade 时不要从其闭包或锁保护区回调信号模块；实现层的锁不可重入。

实现入口：

- [稳定 API](signal-api/api-v0/src/lib.rs)
- [聚合层](src/lib.rs)
- [全局信号服务](signal-impl/impl-core/src/global.rs)
- [进程/线程注册表](signal-impl/impl-core/src/registry.rs)
- [内部状态](signal-impl/impl-core/src/state.rs)
- [timer](signal-impl/impl-core/src/timer.rs)
- [syscall 与 signal frame](../../wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/signal.rs)

## 验证

host 单元测试覆盖 pending 合并、fork/clone/exec 规则、mask 恢复、备用信号栈，以及
interval/POSIX/CPU timer。运行：

```sh
cargo test --manifest-path components/wateros-ipc/Cargo.toml \
  -p wateros-ipc-signal-impl-core
```

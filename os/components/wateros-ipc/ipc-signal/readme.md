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

## 状态所有权

- 进程共享：disposition、process pending、interval/POSIX timer、CPU timer 计数。
- 线程私有：blocked mask、thread pending、`sigsuspend`/poll 临时 mask、
  `sigwait` 等待集合、备用信号栈状态。
- syscall/task：进程树、线程调度状态、wait queue、用户地址空间。

主要生命周期规则：

- `fork`：复制 disposition、调用线程 mask 和备用信号栈；不继承 pending、interval
  timer 或 POSIX timer。
- `CLONE_THREAD`：共享进程状态，新线程继承 mask，不继承备用信号栈。
- `exec`：保留 ignored disposition、pending 和 interval timer；重置用户 handler、
  POSIX timer 与备用信号栈。
- thread exit：删除线程状态；最后一个线程退出时删除进程状态。

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

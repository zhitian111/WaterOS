# ipc syscall

本目录负责 Linux 信号和内核 IPC ABI，底层复用 `wateros-ipc` 的 futex、waitqueue、
SHM 与 signal 状态。

## 当前能力

- 普通信号 mask/action/pending/suspend/timedwait/altstack、kill/tkill/tgkill 与 signal frame。
- signalfd4：批量读取、poll、共享 OFD、pending 事务与 `EFAULT` 回滚。
- eventfd2：普通/信号量模式、阻塞/非阻塞、poll 和 dup/fork 共享。
- futex wait/wake、bitset、requeue/cmp_requeue、robust list、超时与共享映射 key。
- SysV SHM：shmget/shmat/shmdt/shmctl 的常用生命周期。

## 已知边界

- PI futex、`FUTEX_WAKE_OP` 和 realtime signal 队列仍未实现，保持 `ENOSYS`。
- SysV message queue 与 semaphore 尚无 registry、`SEM_UNDO` 和删除唤醒能力。
- 普通信号按位合并；完整 queued siginfo 只在后续 realtime signal 队列中实现。

扩展 futex 时必须同步 scheduler waiter、futex registry key、bitset 和 active-users；
不能只搬 scheduler 队列。锁顺序为短持 IPC registry，解锁后进入 waitqueue/scheduler。

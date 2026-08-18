# signal API v0 离线开发手册

本文说明信号稳定类型、状态边界与扩展方式。模块总览见 [ipc-signal](../../readme.md)，实现见
[impl-core](../../signal-impl/impl-core/README.md)，syscall 接入见
[sys/ipc](../../../../wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/README.md)。

## 1. 分层与安全点

API crate 定义 1..=64 的信号号、位集、action、pending/effect、备用栈和 timer 类型。实现层
维护进程/线程状态；syscall 层处理权限、用户结构和 signal frame；task 层真正中断、停止、
继续或终止任务。

生成信号只写 pending 并返回 `SignalDispatch`。disposition 必须在目标线程返回用户态前的
安全点重新读取，因为排队期间 action 和 mask 都可能变化。不要在 send 时固定 handler。

## 2. 编号、位集与不可变信号

`SignalSet(u64)` 第 `sig-1` 位对应信号 `sig`。非法编号的 `contains` 为 false，insert/remove
无动作；syscall 层必须先用 `valid_signal` 校验，不能把无动作当作合法成功。

普通信号按位合并，同一信号重复生成不累计次数；`first_signal()` 总选最低编号。当前 API
不是 realtime queued-signal 队列，不保存 siginfo 队列和重复实例。

`SIGKILL` 与 `SIGSTOP` 不能被阻塞，也不能改变 disposition。所有 mask 写入路径和 action
mask 都必须清掉这两位。`default_ignored/default_stops/default_terminates` 只编码默认动作；
`SIGCONT` 另有不可被 mask/disposition 抑制的继续副作用。

## 3. `SignalAction` ABI

`#[repr(C)] SignalAction { handler, flags, restorer, mask }` 是 IPC 层 64 位布局视图。
`handler=0/1` 分别为 `SIG_DFL/SIG_IGN`，大于 1 才是用户 handler。支持的 flags 常量包括
`SA_SIGINFO/ONSTACK/RESTART/NODEFER/RESETHAND/RESTORER` 及 NOCLD*。

API 类型不等于可以直接解引用的用户结构。syscall 层必须校验 `sigsetsize`、用户地址与平台
ABI，再 copy-in/out；架构层负责保存寄存器、16 字节栈对齐、参数寄存器和 trampoline。

安装 action 时：不可修改 SIGKILL/SIGSTOP；清理 action.mask 中不可屏蔽位；若新 disposition
为 ignore，应处理该信号已存在的 process/thread pending；未知 flag 的兼容策略由 syscall
明确决定。

## 4. process 与 thread 状态

进程共享 disposition、process pending、线程成员、interval/POSIX timer 和 CPU timer 统计。
线程独有 blocked mask、thread pending、临时等待 mask、sigwait 集与备用栈。

`TaskId`、用户 TID 和 PID 是三个命名空间。线程表以 WaterOS TaskId 索引，send_process 以
PID 路由，用户 `tgkill` 同时校验 TGID/TID；混用会向错误线程投递或在退出时泄漏状态。

生命周期：fork 复制 disposition、调用线程 mask/备用栈但不复制 pending/timer；CLONE_THREAD
共享进程状态，新线程继承 mask；exec 保留 ignored disposition并重置用户 handler、POSIX
timer 与备用栈；线程退出删线程状态，最后线程退出再删进程状态。

## 5. dispatch 与 delivery

```text
kill/tkill/timer/terminal/fault
  -> send_process/send_thread/force_thread_signal
  -> registry 锁内写 pending、选候选 task
  -> 返回 SignalDispatch
  -> 释放 signal registry 锁
  -> syscall/task 层 apply：interrupt/reschedule/stop/continue
  -> 目标安全点 take_deliverable
  -> SignalEffect::{Handler,Terminate,Stop,Continue}
```

`SignalDispatch.target_task_id` 只是锁外副作用建议，不证明任务已经被唤醒。`Ignored` 不做事；
`Pending` 通常 interrupt 并请求重调度；Stop/Continue 要对整个进程状态和成员任务应用效果。

`PendingSignal` 携带交付时 action 快照、原 mask 与 thread/process scope。进入 handler 时，新
mask 通常是旧 mask ∪ action.mask ∪ 当前信号；`SA_NODEFER` 排除最后一项，`SA_RESETHAND`
在选择交付时重置 disposition。`rt_sigreturn` 必须恢复原 mask 与用户现场。

## 6. pending 的 reservation/rollback

普通交付的 `take_deliverable` 会消费 pending 并生成 effect。同步消费者如 signalfd 需要
`take_pending_record`，它返回 `TakenPendingSignal { signal, scope }`。scope 必须保留，因为
user-copy 失败时 `restore_pending_record` 要把位放回原 thread 或 process pending。

正确流程：take record → 锁外 user-copy → 成功提交丢弃记录；失败或 Drop → restore。若只
返回信号号，恢复到错误作用域会改变之后由哪个线程消费；若不恢复则永久丢信号。

重复普通信号本来就按位合并，所以 rollback 与期间新到达的同号信号合并是允许的，但不能
清除后到达的 pending 位。

## 7. 临时 mask 和阻塞 syscall

`sigsuspend`、`ppoll/pselect` 临时 mask 与 `sigwait` 集合都属于线程状态。每个 begin 必须在
成功、超时、EINTR、user-copy fault 和任务退出路径有对应 end/清理；否则线程会永久使用
临时 mask 或继续被当作同步等待者。

等待信号的 lost-wake 协议必须连接三步：registry 登记 wait 状态、task 条件等待、send 返回
后的锁外 interrupt。不能持 signal registry 锁进入 scheduler，也不能先检查 pending 再裸睡眠。

`SA_RESTART` 是否重启 syscall 由 syscall/trap 层根据被中断调用保存的参数决定，API 只携带
flag。不能在 signal registry 内直接修改 trap frame。

## 8. 备用信号栈与 frame

`AlternateSignalStack { sp,size,active_frames }` 的 enable 条件是 size 非零；`contains` 使用
checked_add 防溢出。正在备用栈上时不能替换配置，返回 `AlternateStackActive`。

交付 `SA_ONSTACK` handler 前判断当前 SP 或 `active_frames`，仅在尚未位于备用栈时切换；成功
建立用户 frame 后 `enter_signal_frame`，`rt_sigreturn` 验证并恢复后 `leave_signal_frame`。
user-copy frame 失败时不能错误增加 active_frames，反之成功后遗漏计数会允许嵌套 handler
覆盖正在使用的备用栈。

用户提供的 sp/size 不可信，所有加减、对齐和 frame 大小运算必须 checked，并由 MM user-copy
验证可写范围。

## 9. timer 类型与时钟

`IntervalTimerSpec` 以纳秒保存 interval 与剩余 value，value=0 表示禁用。支持 REAL、VIRTUAL、
PROF；POSIX timer clock 为 Realtime/Monotonic。

- REAL deadline 用单调时间推进，避免 wall clock 修改破坏相对 interval；
- VIRTUAL 只累计用户 CPU 时间；
- PROF 累计用户加内核 CPU 时间；
- POSIX realtime timer 要同时处理 realtime 基准与单调调度；
- 重复 timer 到期推进 deadline 并维护 overrun；旧 deadline 用 generation 过滤。

到期扫描只在 registry 内更新状态并返回 dispatch 列表，调用者释放锁后逐项应用 task 副作用。
SMP 上 CPU timer 必须累计各线程真实运行时间，不能给同一进程简单加全局 wall tick。

## 10. 新增 syscall 实例：`rt_sigtimedwait`

1. copy-in 并校验 64 位 wait set、sigsetsize 与 timespec；清除 SIGKILL/SIGSTOP；
2. 先尝试 `take_pending_record(task_id, wait_set)`；
3. 无记录时 begin_signal_wait，再用条件 wait 封闭登记—入睡窗口；
4. wake、timeout、signal 和所有错误路径都 end_signal_wait；
5. 再 take record，并在锁外构造/copy-out siginfo；
6. copy-out 成功才提交，失败则按 scope restore；
7. 零 timeout 返回 `EAGAIN`，到期返回 `EAGAIN` 或项目既定 Linux 映射，普通中断返回 EINTR；
8. 测试 thread/process pending、重复信号、竞争到达、copy fault、timeout 和多线程消费。

若新增 realtime signals，`SignalSet` 不够：必须设计有界队列、siginfo、排序、资源限制和
fork/exec/exit 清理，不能只复用普通 pending 位。

## 11. 故障与回归

- pending 有但任务不运行：区分 registry 已排队、dispatch 已返回、调用者是否 apply；
- handler mask 不恢复：查 sigreturn 与 temporary mask cleanup；
- signalfd 丢信号：查 record scope 和 user-copy fault rollback；
- SIGKILL 可被屏蔽：查每个 mask 写入口是否清不可变位；
- timer 重复/提前：查时钟基准、generation 和 interval deadline 推进；
- fork-heavy 堆增长：查退出时 thread/process、timer deadline 和 pending source 侧表是否删除；
- signal frame fault 后状态异常：查 pending 消费、active_frames 和来源元数据是否事务化。

回归覆盖 signal 1/64/非法号、mask、默认动作、handler flags、fork/clone/exec/exit、signalfd
rollback、sigsuspend/poll、备用栈、三种 itimer/POSIX timer、SMP 投递，并运行双架构检查。


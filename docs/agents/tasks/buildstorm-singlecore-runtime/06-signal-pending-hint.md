# BS-SC-06：为用户返回增加保守的 signal pending hint

## 任务内容

为每个用户 task 增加 `signal_check_required` 原子提示。`false` 必须表示当前确定不需要进入
signal registry，`true` 允许假阳性；普通无信号返回直接跳过 registry。

## 实施方案

1. hint 由 task 实体持有，通过 task 内部窄 API 设置/消费；ipc-signal 不直接反向依赖 task。
2. 消费使用 `swap(false, AcqRel)`：原值 false 直接返回，true 才调用 `take_deliverable`。
3. sender 顺序固定为：登记 pending → Release 置位 hint → wake/interrupt → reschedule IPI。
4. `apply_signal_dispatch` 对 thread-directed 和 process-directed 目标都设置正确 task 的 hint。
5. 以下状态变化必须重新置位：unblock mask、`sigsuspend`/`ppoll` 临时 mask 开始与恢复、
   `sigaction` 从 ignore/block 变为可投递、signalfd mask 恢复、timer signal、fork/exec 继承。
6. 不缓存精确 deliverable bitmap；mask 与 disposition 变化使其难以无锁保持精确。
7. task id 回收前清理 hint，防止旧 sender 把提示发布给复用后的 task。

竞态必须逐项证明：sender 在 consumer swap 前、swap 后但 registry lock 前、registry 检查中、
检查后各发生一次时都不能丢信号。必要时使用 Loom 风格 host 模型测试或确定性原子状态机测试。

## 涉及文件

- `os/components/wateros-task/task-impl/impl-core/src/tcb.rs`
- `os/components/wateros-task/src/` 中 signal hint facade/lifecycle 文件
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/signal.rs`
- signal mask、sigsuspend、poll 临时 mask、signalfd 和 timer 相关调用点
- `history/06-brief.md`

## CodeGraph 查询

```bash
codegraph explore "send_thread send_process apply_signal_dispatch take_deliverable"
codegraph explore "sigprocmask sigsuspend temporary signal mask signalfd restore"
codegraph callers "apply_signal_dispatch"
codegraph callers "take_deliverable"
codegraph impact "TaskControlBlock"
```

## 验收方式

本任务属于 signal 批次 B，不单独跑完整 BuildStorm。

```bash
cd /home/zhitian/project/WaterOS_buildstorm_singlecore/os
make rv_check
make la_check
make kernel-rv-final
make kernel-la-final
cd ..
git diff --check
```

定向压力至少覆盖 thread/process signal、blocked→unblocked、handler/ignore/default 切换、
`sigsuspend`、`ppoll`、signalfd、timer、SIGKILL、SIGSTOP/SIGCONT；每类循环并发发送与返回，
不得丢一次唤醒或多投递一次不可重复信号。

## 完成后简报

新增 `history/06-brief.md`，写清原子 happens-before 证明、所有 re-arm 点、压力结果和普通无信号
返回的 signal-registry 锁次数，并标记“批次 B 完整验收待任务 07”。

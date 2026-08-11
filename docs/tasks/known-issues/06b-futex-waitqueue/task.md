# K-06B：Futex 与 waitqueue 正确性和成本

## 任务目标

在 K-04 证明 futex wait/wake/requeue 是瓶颈后，减少无效锁和空 wake，并保持无丢失
唤醒、timeout、signal、robust list 和 clear_child_tid 语义。

## 执行前必读

- `docs/tasks/known-issues/06-task-scheduler-futex/task.md`
- `docs/prompts/architecture.md`
- `docs/exports/features/wateros-ipc.md`
- `docs/exports/features/wateros-task.md`
- `docs/todo/perf-ipc-sync.md`

## 已知信息与代码证据

当前实现用原子 sequence 关闭复查到入队窗口：

```rust
let observed = wake_sequence.load(Ordering::Acquire);
let not_woken = || wake_sequence.load(Ordering::Acquire) == observed;
```

任何优化必须保留该 happens-before，不得退回“先读值再无条件 sleep”。

## 涉及文件

- `os/components/wateros-ipc/ipc-futex/`
- `os/components/wateros-ipc/ipc-waitqueue/`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/{futex,robust}.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/wait.rs`

## 任务内容

1. 统计 queue、operator_refs、wait/wake/requeue、空 wake 和等待时长。
2. 审计 wait、wake 与 requeue 的序列更新、queue ID 生命周期和同 key 边界。
3. 将 user memory fault/probe 放 scheduler/registry 锁外。
4. 回收空 queue，但不得在并发 operator 仍持有 queue ID 时复用。
5. 只在 profile 支持时优化索引或锁粒度。

## 如何验收

- [ ] futex LTP、pthread、robust、clear_child_tid、timeout/signal 全部通过。
- [ ] 8 核随机 wait/wake/requeue 无永久睡眠或重复唤醒。
- [ ] 测试后 queue/operator 数回到基线。
- [ ] BuildStorm/CAgent 和双架构 check 通过。

交付 `docs/tasks/history/known-issues/k06b-YYYYMMDD.md`。

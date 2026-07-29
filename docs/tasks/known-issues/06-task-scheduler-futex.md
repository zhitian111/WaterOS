# K-06：Task、Scheduler、Futex 与退出生命周期

## 任务目标

在 K-04 证明调度等待、futex 或 process lifecycle 是 Top 3 后，消除有效任务未运行、
丢失唤醒、队列/registry 无界增长和持锁销毁大对象问题，并改善 lmbench context
switch、fork/exit 与 BuildStorm 并发编译。

## 执行前必读

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`
- `docs/exports/features/wateros-task.md`
- `docs/exports/features/wateros-ipc.md`
- `docs/exports/features/wateros-syscall.md`
- `docs/exports/features/wateros-mm.md`
- `docs/audits/lock-inventory.md`
- `docs/audits/resource-inventory.md`
- `docs/todo/perf-ipc-sync.md`
- `docs/todo/perf-lock-resource.md`
- `docs/todo/perf-fork-exit-degradation.md`

## 已知信息与代码证据

以下优化已经存在，状态是“全量复验待完成”：

- 时间片从历史 100 ms 调为 500 ms；
- stale ready entry 阈值为 8，已有 lazy compact；
- futex 使用 `wake_sequence` 关闭 condition check 到入队之间的丢唤醒窗口；
- CAgent/BuildStorm 已推动多项 wait、timer reschedule、process exit 修复。

当前 process registry 的 reap 仍在可变 registry 操作中 drop 地址空间：

```rust
self.remove_process(pid).map(|process| {
    if let Some(aspace) = process.address_space {
        drop_user_aspace_on_task_exit(aspace.user_aspace_ptr());
    }
    (process.snapshot(), task_ids)
})
```

如果 `with_process_registry` 在整个闭包持锁，这仍是 L-1 的长临界区候选，须用锁等待
和 drop 耗时证据确认。

## 涉及文件

- `os/components/wateros-base/base-config/src/task.rs`
- `os/components/wateros-task/task-scheduler/scheduler-{api,impl}/`
- `os/components/wateros-task/task-impl/impl-core/src/process.rs`
- `os/components/wateros-task/src/{process,cpu}.rs`
- `os/components/wateros-ipc/ipc-futex/`
- `os/components/wateros-ipc/ipc-waitqueue/`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/{task,ipc}/`
- `os/src/trap_handler.rs`
- `docs/todo/perf-{hotpath,ipc-sync,lock-resource,fork-exit-degradation}.md`

## 可并行任务

- [`K-06A：scheduler/lat_ctx`](./06a-scheduler-ctx.md)
- [`K-06B：futex/waitqueue`](./06b-futex-waitqueue.md)
- [`K-06C：process reap`](./06c-process-reap-lifecycle.md)

三项可并行调查。公共 task API 或锁序变化必须先单独合入，再由其它分支 rebase。

## 任务内容

以下子项可由不同 agent 并行调查，但 scheduler/process/futex API 变更必须先单独提交：

1. **ctx 复验**：在 RV/LA、glibc/musl 重跑 `lat_ctx`。若仍为 0，先确认 fork/setup
   失败、超时、非法样本或调度错误，不再盲目增大时间片。
2. **runqueue/IPI**：当 runnable > 0 且 CPU idle 时，跟踪 enqueue owner、远端 IPI、
   pending reschedule 和 pick。维护“task 同时最多属于一个 CPU/队列”的断言。
3. **futex**：测量空 wake、requeue、queue 数和 wait duration；修复必须保持
   `wake_sequence` 的 happens-before 关系、Mesa condition 语义和 timeout/signal。
4. **退出回收**：将 PCB/TCB 从 registry 移除与大对象 drop 分成两阶段；地址空间、
   fd、signal、futex robust cleanup 不得在全局 registry 自旋锁内销毁。
5. **资源有界**：循环 fork/exit 后 process/task/futex/waitqueue/versions 数量回到
   基线；PID/TID 或 fd 位图只有 profile 证明线性分配占比高时才实施。
6. **接口边界**：task API 返回拥有所有权的待 drop 对象或 cleanup token，MM/VFS
   回调在锁外执行，不能反向获取 process registry。

推荐的两阶段形状：

```rust
let retired = with_process_registry(|registry| registry.detach_exited(pid))?;
retired.drop_address_space_and_resources(); // registry 锁外
```

实际类型名按现有 API 调整；禁止把裸指针生命周期扩散到 syscall。

## 如何验收

- [ ] `make rv_check && make la_check` 通过。
- [ ] 四配置可运行的 `lat_ctx` 均产生有效值，修改前后三轮数据可比。
- [ ] 8 核远端 wake 压测无 runnable-but-idle、重复运行或 task 丢失。
- [ ] futex wait/wake/requeue、robust list、clear_child_tid 和 signal interruption LTP
      通过，无永久睡眠。
- [ ] 10,000 次 fork/exit 后所有 registry/queue 数量有界，heap 使用回到稳定区间。
- [ ] 地址空间和大资源 drop 发生在 registry/scheduler 锁外，锁序审计已更新。
- [ ] BuildStorm 完整成功且 CAgent 三轮 10/10。

每个保留子项写 `docs/tasks/known-issues/results/k06-<subtask>-YYYYMMDD.md` 并独立提交。

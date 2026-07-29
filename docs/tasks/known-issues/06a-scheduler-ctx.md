# K-06A：Scheduler、runqueue 与 context-switch 复验

## 任务目标

复验四配置 `lat_ctx`，并在 K-04 证明 scheduler 是瓶颈时修复 runnable-but-idle、
stale queue 或抢占问题。该任务不修改 futex registry 和地址空间实现。

## 执行前必读

- `docs/tasks/known-issues/06-task-scheduler-futex.md`
- `docs/prompts/architecture.md`
- `docs/exports/features/wateros-task.md`
- `docs/tasks/perf/wave1-fix-ctx-switch-zero-score.md`

## 已知信息与代码证据

当前时间片和 stale 阈值已经调整：

```rust
pub const MAX_TICKS_PER_TASK: u64 = 50;
pub const READY_QUEUE_STALE_COMPACT_THRESHOLD: usize = 8;
```

因此旧 0 分不能直接通过继续增大时间片处理。

## 涉及文件

- `os/components/wateros-base/base-config/src/task.rs`
- `os/components/wateros-task/task-scheduler/`
- `os/src/trap_handler.rs`
- `os/components/wateros-platform/src/lib.rs`

## 任务内容

1. 区分 lat_ctx fork/setup 失败、timeout、无效样本和调度错误。
2. 记录 per-CPU runqueue、pick stale、context switch、idle-with-runnable 和 IPI。
3. 修复时维护 task 单 CPU ownership、状态转换和远端 reschedule 不变量。
4. 不通过 benchmark 名称切换调度策略；调度参数改变须有通用负载回归。

## 如何验收

- [ ] 可运行四配置的 lat_ctx 均有有效值和三轮数据。
- [ ] 8 核 wake/steal/exit 压测无 task 丢失、重复运行或永久 idle。
- [ ] timer、priority、affinity 和 scheduler LTP 无回归。
- [ ] 双架构 check 和完整 BuildStorm 通过。

交付 `docs/tasks/known-issues/results/k06a-YYYYMMDD.md`。

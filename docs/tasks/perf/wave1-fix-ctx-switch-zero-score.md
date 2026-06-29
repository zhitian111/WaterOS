# 性能任务：修复 lmbench context switch 计 0 分（G3）

## 任务目标

使 lmbench `lat_ctx` 在 **LA-glibc（8 项全 0）** 与 **musl-rv（部分 0）** 下产出有效延迟值（score ≥ 1.0），而非失败/超时。

优先 **低风险** 改动：减少微基准被 timer 抢占、ready 队列 stale 扫描放大；不在本任务默认做页表结构 COW（见 `wave3-fork-exit-deep-opt.md`）。

## 背景（必读）

- `docs/todo/perf-baseline-gap-report.md` §G3
- 能跑出分的 64/96 项 latency 很高（8703µs），说明部分失败是 **setup 超时/无效样本** 而非单纯慢

## 执行前必须参考的 prompt

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`

## 执行前必须参考的文档

- `docs/todo/perf-hotpath.md`（H-11、H-13）
- `docs/todo/perf-fork-exit-degradation.md`（D1/D2 会间接影响 ctx setup）
- `docs/tasks/run_testsuits_qemu.md`（P3 lmbench 阶段）

## 需要优先查看的源文件

| 文件 | 用途 |
|------|------|
| `os/components/wateros-base/base-config/src/task.rs` | `SCHED_TIMER_PERIOD_MS`、`MAX_TICKS_PER_TASK`（≈100ms 时间片） |
| `os/src/trap_handler.rs:258-268` | timer tick → `schedule_tick` |
| `os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/scheduler.rs:327-366` | tick 抢占 |
| `os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/queues.rs:78-92` | pick_next 跳过 stale |
| `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/clone.rs` | fork N 子进程成本 |

## 实施要点（按优先级尝试）

1. **H-11**：评估 lmbench/短生命周期进程密集时是否应增大时间片或 idle 时跳过 tick promote（可用 compile-time 或运行时启发，避免破坏 LTP 调度语义）。
2. **H-13**：`pick_next` 时若连续跳过过多 stale entry，触发 lazy compact 或限制 `ready_queue` 中 stale 比例。
3. 确认 fork N 失败时 lmbench 是否打印错误；若为 ENOMEM/超时，记录日志并考虑与 D2 泄漏修复联动验证。
4. **不要**在本任务改 trap/TLB（见 wave3）。

## 验收标准

- [ ] `make rv_check && make la_check` 通过
- [ ] P3 跑 lmbench（至少 glibc RV + LA 各一次），`lat_ctx` 不再大面积 score=0
- [ ] 无 LTP 调度/定时器明显回归（抽样或用户确认）
- [ ] 改动有注释说明对 micro-benchmark 与生产路径的权衡

## 诊断步骤（实施前建议）

1. grep 日志 `lat_ctx`、`fork failed`、`timeout`
2. 对比 N=2（有分）与 N=64（0 分）时 ready_queue 长度、fork 返回值

## 完成后的回填

- **已合入（2026-06-29）**：
  - **H-11**：`WaitQueues::has_due_timers`；Tick 路径仅在 quantum 耗尽 / RT 抢占 / 到期 timer 时 `promote_sleep_and_timeouts`（multi-class + round-robin）。
  - **H-13**：`READY_QUEUE_STALE_COMPACT_THRESHOLD` + `pick_next` lazy compact（multi-class + round-robin）。
  - **时间片**：`MAX_TICKS_PER_TASK` 10→50（500ms），降低微基准非自愿抢占。
  - **D2**：`forget_task` 已在 `8c58776` 合入，本次未重复改动。
- **RV glibc 最小验收**（`test_case/sdcard-rv.img`，仅 `lat_ctx`）：修复版与基线均产出 8 项有效 µs（见 `os/lat_ctx_minimal_fix.log`、`os/lat_ctx_minimal_base.log`）。N≤32 延迟更低；全量 `lmbench_testcode.sh` / LA 仍待测。
- 记录哪条改动消除了 0 分（抢占 vs 队列 vs fork 失败）

## 示例：交给 Agent 的一次性用户 prompt

```
@docs/tasks/perf/wave1-fix-ctx-switch-zero-score.md

请修复 lmbench context switch 计 0 的问题，优先时间片/抢占与 ready 队列 stale。
make rv_check && la_check，再跑 P3 lmbench 验证 lat_ctx。
不要做 trap/TLB 大改。
```

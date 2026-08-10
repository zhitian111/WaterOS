# 任务运行时统计轻量查询（2026-08-11）

## 为什么选择这里

当前 pc-hot 中：

```text
TaskRegistry::task_snapshot / current_task_snapshot / process_snapshot 合计约 500M+
```

`times(2)`、`getrusage(2)` 和 `clock_gettime(CLOCK_PROCESS_CPUTIME_ID)` 都只需要
当前任务的 `tick_count`，但它们现在都调用 `current_task_snapshot()`，构造并复制
整个 `TaskSnapshot`，包括 trap frame、地址空间 token、亲和性、调度状态等大量字段。

## 选择的方案

增加轻量查询：

```text
TaskControlBlock::runtime_stats()
TaskRegistry::task_runtime_stats()
MultiClassScheduler::current_task_runtime_stats()
task::current_task_runtime_stats()
```

只返回 `TaskRuntimeStats`，并保留 `current_task_snapshot` 中“当前运行中 live tick
delta”的语义。`times`、`getrusage`、`CLOCK_PROCESS_CPUTIME_ID` 改为使用新 API。

## 为什么这么做

1. 只减少调用方不需要的大快照复制，不改变调度器状态或任务生命周期。
2. 新增 API 是纯增量，不替换现有 `current_task_snapshot`。
3. BuildStorm 会频繁调用进程时间统计，这个路径有明确的指令收益。

## 接下来的工作

1. 在 `perf/task-lightweight-runtime-stats` 分支实现。
2. 双架构 Final check。
3. 180 秒 smoke。
4. RISC-V 完整 BuildStorm A/B；相对当前 main 有 ≥ 1.5% 净改善才合并。
5. 完成后补 pc-hot/wait-hot 并归档。

## 验收标准

- 双架构 Final check 通过。
- `times`、`getrusage`、`clock_gettime` 语义无回归。
- 完整 BuildStorm 无 panic/SIGSEGV，相对同宿主 main 有可复现收益。

## 实测结果（2026-08-11）

```text
task-runtime-stats-full-a1: BUILDSTORM_COMPILE ok=true elapsed_s=809.22
task-runtime-stats-full-a2: cagent 结束后停滞，未进入 BuildStorm，1200s 超时
main-cow-full-b1:           BUILDSTORM_COMPILE ok=true elapsed_s=817.27
```

首轮快约 1.0%，但第二轮在 cagent 结束后长时间停滞，不可验收。代码已全部回退，
仅保留本记录。

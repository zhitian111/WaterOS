# K-06A 当前任务实时运行统计报告（2026-08-01）

## 问题

初赛镜像原生 `getrusage04` 持续 busy loop 并查询 `RUSAGE_THREAD`。调度器把当前任务
新增的 tick 暂存在 `CPUState.current_runtime_ticks`，只在任务离开 CPU 时回写 TCB；
`current_task_snapshot()` 只返回 TCB 快照。因此任务未发生切换时，`getrusage()` 会永久
返回同一个值，LTP 无法收集 20 次时间增量。

## 修复

`MultiClassScheduler::current_task_snapshot()` 在 scheduler 同一临界区内，把当前 CPU
尚未回写的 runtime tick 合并到快照，并用 CPU cache 中的实时 vruntime 覆盖 CFS
任务的旧值。TCB 仍是持久化统计所有者，查询不会修改或重复回写计数。

同时修正 `bringup-ltp-glibc-only` / `bringup-ltp-musl-only`：两者现在都通过对应镜像的
busybox 执行 `ltp_testcode.sh`；旧 glibc 入口误跑 lmbench，旧 musl 入口因 PATH 中无
`sh` 返回 127。

## 验证

- `make rv_check`：通过。
- RISC-V64/OpenSBI/8 CPU，初赛镜像原生 `getrusage04`：5 个全新 qcow2 overlay，
  每轮连续执行 20 次；共 100 次 `TPASS`，5 轮顶层命令均正常结束。
- 完整 musl LTP 首轮在修复前推进到第 839 项 `getrusage04`；修复后的完整重跑另行记录。

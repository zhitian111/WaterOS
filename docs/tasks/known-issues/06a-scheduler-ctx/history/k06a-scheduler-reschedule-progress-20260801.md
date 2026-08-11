# K-06A Scheduler 重调度与 yield 进展报告（2026-08-01）

## 结果

- `ScheduleReason::Reschedule` 不再用 Tick 条件二次检查。调用方已经消费
  `need_resched`，旧实现会把 `Forced` 请求错误丢弃。
- CFS/Idle 类任务执行 `sched_yield()` 时，若已有同类 ready task，将当前任务的
  vruntime 移到 ready 最小值之后，避免当前任务因 vruntime 较小而立即再次选中。
- 未改变 FIFO/RR 的 yield 语义，也未按测试名称切换调度策略。

## 验证

- `make rv_check`：通过。
- `make la_check`：通过。
- RISC-V64、OpenSBI、8 CPU、初赛镜像原生
  `/musl/ltp/testcases/bin/epoll-ltp` 后接 `exit_group01`：修复候选整体连续三轮通过；
  每轮 epoll_ctl 13,824 项全过，`exit_group01` 为 `TPASS`，顶层 shell 正常退出。
- `cargo test -p wateros-task-scheduler-api-v0` 无法在 x86_64 host 运行：依赖图中的
  `sbi-rt` 使用 RISC-V `a0..a7` 内联寄存器。新增的两个局部测试已通过双架构
  `cargo check` 编译，但没有伪造 host 测试结果。

## 涉及文件

- `os/components/wateros-task/task-scheduler/scheduler-api/api-v0/src/cpu.rs`
- `os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/scheduler.rs`

完整初赛 LTP、CAgent 与 BuildStorm 仍属于 K-10 回归门禁。

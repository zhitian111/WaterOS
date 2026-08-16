# BS-SC-03 任务简报

## 状态

接受，批次 A 完整功能与性能验收待 BS-SC-04。commit 为本简报所在提交，完整 hash 由下一
任务补记。

## 修改与影响面

- scheduler 在原有 `set_timekeeper_cpu` 锁内状态之外，以 Release 顺序发布只读原子 CPU id；
- task 组合层新增 `is_timekeeper_cpu` 窄 facade，不修改 scheduler/task `api-v0`；
- signal timer 的单调时钟读取、per-CPU elapsed 和当前进程 `account_cpu` 仍在所有 CPU 执行；
- `expire_realtime`、realtime clock 读取和 `expire_posix_timers` 仅在 timekeeper CPU 执行；
- dispatch 仍收集后在 signal registry 锁外应用，scheduler tick、10ms 配置和 timer 重武装未改。

CodeGraph 确认 `expire_realtime`/`expire_posix_timers` 只从 syscall signal timer 调用，并展开
`schedule_tick -> schedule -> is_timekeeper_cpu`。scheduler 已有唯一、不可迁移的 timekeeper
约束，因此原子快照没有引入第二套选举策略。

## 调用次数

结构门控后，每个名义 timer 周期的全局 expiration 扫描从“每个 online CPU 一次”变为
“timekeeper 一次”：RV 8 vCPU 从最多 8 次降为 1 次，LA 12 vCPU 从最多 12 次降为 1 次。
`LAST_ACCOUNTING_NS[cpu]` 和 `account_cpu` 仍分别发生在 8/12 个 CPU 的本地 tick 路径。
本提交未加入常驻诊断计数；实际 timer 精度、单次触发和 CPU accounting 回归由 BS-SC-04
guest 测试关闭。

## 实际验收

```text
make rv_check                         PASS（仅有既有 warning）
make la_check                         PASS（仅有既有 warning）
make kernel-rv-final                  PASS
make kernel-la-final                  PASS
git diff --check                      PASS
```

## 决定与剩余风险

保留该低风险提交。主要剩余风险是 timekeeper 条件错误会造成 wall timer 漏触发，或多个 CPU
同时触发会重复投递；BS-SC-04 必须在两架构覆盖 setitimer/POSIX timer 的一次性、周期性和
精度，并确认 CPU timer 未停止。已同步 signal 与 scheduler README 中的并发契约；架构、
feature 和用户构建接口未变化。

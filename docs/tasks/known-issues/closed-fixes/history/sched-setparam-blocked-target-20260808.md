# `sched_setparam` 支持 Blocking/Sleeping 目标进程（2026-08-08）

## 问题

LTP `sched_setparam03` 中，子进程调用 `sched_setparam(getppid(), priority=5)`
设置父进程的实时优先级。父进程此时在 `waitpid` 中阻塞，内核调度策略修改路径把
`TaskState::Blocking/Sleeping` 与 `Exited` 一起判为 `NoSuchTask`，因此错误返回
`ESRCH`，父进程优先级也未被更新。

临时解析日志确认 `getppid()` 返回的 PID 能通过进程注册表解析到 task，失败发生在
调度器状态判断：

```text
[sched-resolve] pid=5 thread_task=Some(38) leader_task=Some(38)
sched_setparam03.c:31: TFAIL: sched_setparam(getppid(), &p5) failed: ESRCH
```

## 修改

`os/components/wateros-task/task-scheduler/scheduler-impl/impl-multi-class/src/scheduler/policy.rs`：

- `TaskState::Blocking(_)` / `TaskState::Sleeping { .. }` 允许更新
  `sched_policy/sched_priority`。
- 更新后不重新入队、不触发 reschedule；唤醒路径会按新的 TCB 调度属性入队。
- 只有 `TaskState::Exited(_)` 继续返回 `NoSuchTask`。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

RISC-V LTP 定向日志 `/tmp/sched-regression-fixed.log`：

- `sched_setparam01`：passed 2，failed 0
- `sched_setparam02`：passed 8，failed 0
- `sched_setparam03`：passed 10，failed 0
- `sched_setparam04`：passed 18，failed 0
- `sched_setscheduler01`：passed 26，failed 0
- `sched_setscheduler03`：passed 32，failed 0

LoongArch 使用同一组用例复验，日志 `/tmp/sched-regression-la-fixed.log`：

- `sched_setparam01`：passed 2，failed 0
- `sched_setparam02`：passed 8，failed 0
- `sched_setparam03`：passed 10，failed 0
- `sched_setparam04`：passed 18，failed 0
- `sched_setscheduler01`：passed 26，failed 0
- `sched_setscheduler03`：passed 32，failed 0

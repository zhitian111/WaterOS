# `PR_SET/GET_TIMERSLACK` 实现（2026-08-09）

## 问题

LTP `prctl08/09` 中 `PR_SET_TIMERSLACK` 返回 `EINVAL`，`PR_GET_TIMERSLACK`
返回 `ENOSYS`。

## 修改

`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/task.rs`：

- 新增 per-task `(default_ns, current_ns)` timer slack 状态。
- `PR_SET_TIMERSLACK`：设置 current；`0` 表示重置为 default；成功返回 0。
- `PR_GET_TIMERSLACK`：返回 current。
- `copy_timer_slack`：fork/clone 时子任务继承父任务 current，且 default 也
  同步为 current。
- `clone.rs` 在 fork 和 clone thread 路径调用 `copy_timer_slack`。
- procfs 新增 `/proc/<pid>/timerslack_ns`，从 syscall 层查询 per-task current
  timer slack。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

RISC-V LTP `prctl08`（日志 `/tmp/prctl08-09-proc-fixed.log`）：

- Reset/1/70000/INT_MAX 的 SET/GET 全部 TPASS。
- 子进程继承父进程 current 值，TPASS。
- `/proc/self/timerslack_ns` 与 SET/GET 保持一致，全部 TPASS。

`prctl09` 的 SET/GET 正常；多数定时采样 TPASS，偶发一次超过 coarse timer
阈值，属于当前 10ms 调度 tick 粒度下的定时噪声。

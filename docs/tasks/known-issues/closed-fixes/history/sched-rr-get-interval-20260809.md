# `sched_rr_get_interval(2)` 实现（2026-08-09）

## 问题

RISC-V64/LoongArch64 的 `__NR_sched_rr_get_interval` 为 127，此前未注册，LTP
`sched_rr_get_interval01..03` 返回 `ENOSYS`。

## 修改

- `os/components/wateros-syscall/syscall-api/api-v0/src/number.rs`：新增
  `SCHED_RR_GET_INTERVAL=127`。
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/syscall_nr_dispatch.rs`：
  分发到 `sys_sched_rr_get_interval`。
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/sched.rs`：
  - 目标策略为 `SCHED_RR` 时返回 `MAX_TICKS_PER_TASK × 10ms = 500ms`。
  - 其他策略返回 `{0,0}`，匹配 Linux/LTP 对 `SCHED_FIFO` 的语义。
  - `pid<0` 返回 `EINVAL`，不存在 pid 返回 `ESRCH`，坏指针返回 `EFAULT`。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

RISC-V LTP 定向日志 `/tmp/sched-rr-fixed2.log`：

- `sched_rr_get_interval01`：RR 返回 `0s 500000000ns`，TPASS。
- `sched_rr_get_interval02`：FIFO 返回 0 时间片，TPASS。
- `sched_rr_get_interval03`：`EINVAL/ESRCH/EFAULT` 路径 TPASS。

LoongArch LTP 定向日志 `/tmp/sched-rr-la-fixed.log` 同样全部通过。

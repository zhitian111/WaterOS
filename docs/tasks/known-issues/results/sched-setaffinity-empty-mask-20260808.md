# `sched_setaffinity` 空 CPU mask 返回 `EINVAL`（2026-08-08）

## 问题

LTP `sched_setaffinity01` 使用全零 CPU mask 调用 `sched_setaffinity`，期望
`EINVAL`。内核此前允许空集合，`sched_setaffinity` 错误返回成功。

## 修改

`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/sched.rs`：

- `CpuMask::try_from_le_bytes` 成功且 `mask.bits() == 0` 时返回 `EINVAL`。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

LTP 定向日志 `/tmp/schedaff-einval-fixed.log`：

```text
sched_setaffinity01: EFAULT / EINVAL / ESRCH 全部 TPASS
```

剩余一项：`EPERM` 用例仍错误返回成功，需要继续检查 fork 后目标进程凭据查询。

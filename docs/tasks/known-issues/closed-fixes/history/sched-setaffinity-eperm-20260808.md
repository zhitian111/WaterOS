# `sched_setaffinity` 无权限调用返回 `EPERM`（2026-08-08）

## 问题

LTP `sched_setaffinity01` 的第四项先用 `fork()` 创建目标进程，再通过
`seteuid(nobody)` 降低调用者权限，最后对该目标进程设置 affinity。内核此前在
`can_change_affinity()` 中同时比较调用者的 real UID 与 effective UID；root 进程
只把 effective UID 改为 `nobody` 后，real UID 仍为 0，因此错误放行了本应返回
`EPERM` 的调用。

## 修改

`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/sched.rs`：

- `can_change_affinity()` 改为只允许：
  - 调用者 effective UID 为 0；
  - 调用者 effective UID 等于目标进程 real UID；
  - 调用者 effective UID 等于目标进程 effective UID。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

RISC-V LTP 定向日志 `/tmp/schedaff-eperm-fixed.log`：

```text
sched_setaffinity01.c:83: TPASS: sched_setaffinity() failed: EFAULT (14)
sched_setaffinity01.c:83: TPASS: sched_setaffinity() failed: EINVAL (22)
sched_setaffinity01.c:83: TPASS: sched_setaffinity() failed: ESRCH (3)
sched_setaffinity01.c:83: TPASS: sched_setaffinity() failed: EPERM (1)

Summary:
passed   4
failed   0
broken   0
skipped  0
warnings 0
```

`sched_setaffinity01` 四项错误语义全部通过。

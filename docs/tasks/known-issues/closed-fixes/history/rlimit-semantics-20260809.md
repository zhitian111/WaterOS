# rlimit 语义修复与 `prlimit64(pid != 0)`（2026-08-09）

## 问题

- `getrlimit` 对非法 resource 返回成功，应返回 `EINVAL`。
- `setrlimit` 对 `rlim_cur > rlim_max` 返回成功，应返回 `EINVAL`。
- `setrlimit(RLIMIT_NOFILE)` 允许超过 `NR_OPEN`，应返回 `EPERM`。
- `prlimit64` 只支持当前进程，任意非零 pid 都返回 `ESRCH`。

## 修改

`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/rlimit.rs`：

- 增加 `RLIM_NLIMITS=16` 与 `NR_OPEN=1024*1024`。
- `getrlimit`/`setrlimit`/`prlimit64` 对非法 resource 返回 `EINVAL`。
- 设置 rlimit 时校验 `cur <= max`，否则 `EINVAL`。
- `RLIMIT_NOFILE` 的 `max > NR_OPEN` 返回 `EPERM`。
- `prlimit64` 支持非零 pid：解析目标进程、读取其 rlimit、按 root/同 uid
  权限允许修改。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

RISC-V LTP 定向日志 `/tmp/rlimit-regression-fixed.log`：

- `getrlimit01`：16 个资源全部 TPASS。
- `getrlimit02`：`EFAULT`/`EINVAL` TPASS。
- `getrlimit03`：`prlimit64` 与 `getrlimit` 对资源 0..15 结果一致。
- `setrlimit03`：`EPERM`/`EINVAL` TPASS。
- `setrlimit04/05`：继承与 `EFAULT` TPASS。

LoongArch LTP 定向日志 `/tmp/rlimit-regression-la-fixed.log` 同样全部通过。

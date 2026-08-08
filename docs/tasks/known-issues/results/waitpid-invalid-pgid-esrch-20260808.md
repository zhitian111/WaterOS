# `waitpid` 无效进程组返回 `ESRCH`（2026-08-08）

## 问题

LTP `waitpid04` 对 `waitpid(INT_MIN, NULL, 0)` 期望 `ESRCH`。内核把负 PID 直接
转换为 `ProcessGroup`，随后因找不到子进程返回 `ECHILD`。

## 修改

`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/task/wait.rs`：

- `pid < -1` 时先检查绝对 pgid 是否超过 `i32::MAX`。
- 无效 pgid 直接返回 `ESRCH`，不再进入进程组等待。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

LTP 定向日志 `/tmp/waitpid04-esrch-fixed.log`：

```text
waitpid04: ECHILD / ECHILD / EINVAL / ESRCH 全部 TPASS
```

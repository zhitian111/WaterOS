# select/pselect 已关闭 fd 返回 `EBADF`（2026-08-08）

## 问题

LTP `pselect02` 在 `fd_set` 中放入一个已关闭 fd 并调用 `pselect`，期望 `EBADF`。
共享 `select/pselect` 扫描把 `POLLNVAL` 当普通就绪位处理，错误地返回成功。

## 修改

`os/components/wateros-syscall/syscall-impl/impl-kernel/src/poll_engine.rs`：

- `scan_fd_sets_inner` 对受监控 fd 的 `poll_revents_fd` 返回 `POLLNVAL` 时直接返回
  `EBADF`。
- 仅对出现在 `readfds` / `writefds` / `exceptfds` 中的 fd 生效，不改变未监控 fd
  行为。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

LTP 定向日志 `/tmp/pselect-ebadf-fixed.log`：

```text
pselect02: EBADF / EINVAL x2 全部 TPASS
pselect02_64: EBADF / EINVAL x2 全部 TPASS
```

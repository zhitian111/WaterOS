# `utimensat` NULL pathname 返回 `EFAULT`（2026-08-08）

## 问题

LTP `utimes01` 对 `utimes(NULL, times)` 期望 `EFAULT`。旧逻辑在 `pathname=NULL`
且 `dirfd=AT_FDCWD` 时返回 `EBADF`。

## 修改

`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/attr.rs`：

- `pathname=NULL` 且 `dirfd=AT_FDCWD` 时直接返回 `EFAULT`。
- 保留 `dirfd>=0` 时基于 fd 的 `utimensat` 路径。
- 尝试在时间写入前检查路径是否可写，使只读挂载路径优先返回 `EROFS`。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

LTP 定向日志 `/tmp/utimes-fixed.log`：

```text
utimes01: SUCCESS x2 / EACCES / ENOENT / EFAULT / EPERM 全部 TPASS
```

剩余一项：`mntpoint/file` 的只读挂载写入仍返回 `EPERM` 而不是 `EROFS`，需继续跟踪
只读挂载表的路径路由。

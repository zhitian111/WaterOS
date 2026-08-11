# `utimensat` NULL pathname 与只读挂载错误（2026-08-08）

## 问题

LTP `utimes01` 对 `utimes(NULL, times)` 期望 `EFAULT`。旧逻辑在 `pathname=NULL`
且 `dirfd=AT_FDCWD` 时返回 `EBADF`。

## 修改

`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/attr.rs`：

- `pathname=NULL` 且 `dirfd=AT_FDCWD` 时直接返回 `EFAULT`。
- 保留 `dirfd>=0` 时基于 fd 的 `utimensat` 路径。
- 在权限检查前先检查路径是否可写，使只读挂载路径优先返回 `EROFS`，而不是被
  非 root 权限检查提前拦截成 `EPERM`。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

LTP 定向日志 `/tmp/utimes-fixed.log`：

```text
utimes01: SUCCESS x2 / EACCES / ENOENT / EFAULT / EPERM / EROFS 全部 TPASS
FAIL LTP CASE utimes01 : 0
```

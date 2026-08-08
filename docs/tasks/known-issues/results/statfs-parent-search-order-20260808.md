# `statfs` 父目录权限检查顺序（2026-08-08）

## 问题

`pathconf(3)` 底层使用 `statfs`。当不可搜索的父目录后面还有非目录分量时，内核先
展开符号链接，`resolve_symlink_path_with` 会先遇到非目录并返回 `ENOTDIR`，导致
LTP `pathconf02` 的 `EACCES` 用例失败。

## 修改

`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/statfs.rs`：

- 在 `resolve_symlinks` 之前先执行 `check_parent_search`。
- 这样不可搜索父目录优先返回 `EACCES`；普通非目录路径仍返回 `ENOTDIR`，符号链接
  循环仍在后续解析中返回 `ELOOP`。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

LTP 定向日志 `/tmp/pathconf-eacces-fixed.log`：

```text
pathconf02: ENOTDIR / ENOENT / ENAMETOOLONG / EINVAL /
EACCES / ELOOP 全部 TPASS
```

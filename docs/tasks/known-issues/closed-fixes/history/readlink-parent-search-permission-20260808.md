# `readlink` 父目录搜索权限检查（2026-08-08）

## 问题

LTP `readlink03` 第一个用例以 `nobody` 读取位于 `mode=0444` 目录中的符号链接，
期望 `EACCES`。内核此前不检查父目录搜索权限，`readlink` 直接成功。

## 修改

`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/dir.rs`：

- `sys_readlinkat` 解析绝对路径后，逐级检查目标之外的所有父目录是否允许
  `execute/search`。
- 非 root 用户遇到 `mode & 0o111 == 0` 的父目录返回 `EACCES`；中间分量不是目录
  返回 `ENOTDIR`，不存在返回 `ENOENT`。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

LTP 定向日志 `/tmp/readlink-eacces-fixed.log`：

```text
readlink03: EACCES / EINVAL x2 / ENAMETOOLONG / ENOENT /
ENOTDIR / ELOOP / EFAULT 全部 TPASS
FAIL LTP CASE readlink03 : 0
```

# `stat/statx` 父目录搜索权限检查（2026-08-08）

## 问题

LTP `stat03` 以 `nobody` 访问位于 `mode=0666` 目录中的文件，期望 `EACCES`。
`fstatat/statx` 此前不检查父目录搜索权限，直接返回成功。

## 修改

`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/fstat.rs`：

- 新增 `check_stat_parent_search`，逐级检查目标之外的所有父目录 `execute/search`
  权限。
- 非 root 用户遇到 `mode & 0o111 == 0` 的父目录返回 `EACCES`；中间分量不是目录
  返回 `ENOTDIR`，不存在返回 `ENOENT`。
- `fstatat` 与 `statx` 均走该检查。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

LTP 定向日志 `/tmp/stat03-eacces-fixed.log`：

```text
stat03: EACCES / EFAULT / ENAMETOOLONG / ENOENT / ENOTDIR / ELOOP 全部 TPASS
stat03_64: 同样全部 TPASS
```

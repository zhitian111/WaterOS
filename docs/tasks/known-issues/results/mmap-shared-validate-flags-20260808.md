# `MAP_SHARED_VALIDATE` 非法 flag 返回 `EOPNOTSUPP`（2026-08-08）

## 问题

LTP `mmap20` 使用 `MAP_SHARED_VALIDATE | INVALID_FLAG` 调用 `mmap`，期望
`EOPNOTSUPP`。内核此前忽略未识别的 flag，错误地允许映射。

## 修改

`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/mem/mmap.rs`：

- `sys_mmap` 入口检查已知 Linux mmap flags 集合。
- 出现未知 flag 且低两位为 `MAP_SHARED_VALIDATE` 时返回 `EOPNOTSUPP`；
  其它未知 flag 返回 `EINVAL`。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

LTP 定向日志 `/tmp/mmap20-eopnotsupp-fixed.log`：

```text
mmap20: mmap() failed with errno set to EOPNOTSUPP
```

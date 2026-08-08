# user-copy 无效地址统一返回 `EFAULT`（2026-08-08）

## 问题

`copy_from_user` / `copy_to_user` 遇到超出用户 VA 范围或非法地址时，底层返回
`MmError::InvalidAddress`，全局 `mm_err_to_errno` 映射为 `EINVAL`。LTP
`statfs02` 的坏 `path` / 坏 `buf` 指针因此返回错误码不匹配。

## 修改

`os/components/wateros-syscall/syscall-impl/impl-kernel/src/user_copy.rs`：

- 新增 `mm_user_copy_errno`：`InvalidAddress` 统一映射为 `EFAULT`。
- `copy_from_user`、`copy_from_user_in_aspace`、`copy_to_user_progress`、
  `copy_to_user_struct_in_aspace`、`copy_user_path_cstr` 使用该映射。
- 全局 `mm_err_to_errno` 不变，`mmap` / `mprotect` 等非拷贝路径仍保留原有
  `EINVAL` 语义。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

LTP 定向日志 `/tmp/statfs-efault-fixed.log`：

```text
statfs02: ENOTDIR / ENOENT / ENAMETOOLONG / EFAULT x2 / ELOOP 全部 TPASS
statfs02_64: 全部 TPASS
```

## 后续

该映射可能同时改善 `readlink03`、`utimes01` 等依赖坏指针返回 `EFAULT` 的用例，
后续 LTP 回归继续观察。

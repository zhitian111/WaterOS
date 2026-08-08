# `rmdir` 错误语义修复（2026-08-08）

## 问题

LTP `rmdir02` 的 9 个错误路径此前有多处映射错误：

- 非空目录返回 `EEXIST`，应为 `ENOTEMPTY`。
- 符号链接循环返回 `ENOTDIR`，应为 `ELOOP`。
- 删除挂载点返回底层 `EROFS`，应为 `EBUSY`。
- `rmdir(".")` 被当作非空目录，应为 `EINVAL`。

## 修改

错误链路补全：

- `ErrNo::ENOTEMPTY`、`FsError::NotEmpty`、`VfsError::NotEmpty`。
- `ramfs`、`tmpfs`、`another-ext4`、`ext4_rs` 的非空目录删除统一返回
  `FsError::NotEmpty`。
- `FsBridge`、`vfs_util`、两个架构的 ELF root-volume 错误映射补上该错误。

路径与挂载语义：

- `unlinkat` 删除前先用 `resolve_symlinks(..., NoFollow)` 展开中间符号链接，
  使 `ELOOP` 能正确穿透到 syscall。
- `rmdir(".")` / `rmdir("..")` 直接返回 `EINVAL`。
- 新增 `vfs::is_mount_point_absolute()`；`unlinkat(AT_REMOVEDIR)` 删除挂载点时
  返回 `EBUSY`。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

LTP `rmdir02` 定向日志 `/tmp/rmdir2-ebusy-fixed.log`：

```text
ENOTEMPTY / ENAMETOOLONG / ENOENT / ENOTDIR / EFAULT /
ELOOP / EROFS / EBUSY / EINVAL 全部 TPASS
FAIL LTP CASE rmdir02 : 0
```

## 后续

同一轮还复验了 `kill11`、`mmap01`、`socketpair01`、`rmdir01`，当前均通过。

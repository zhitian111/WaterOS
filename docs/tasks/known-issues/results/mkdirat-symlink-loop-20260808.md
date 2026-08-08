# `mkdirat/mknodat` 中间符号链接展开（2026-08-08）

## 问题

LTP `mkdirat02` 对包含 43 层符号链接循环的路径执行 `mkdirat`，期望 `ELOOP`。
内核此前只做路径规范化，没有展开中间符号链接，错误返回 `ENOTDIR`。

## 修改

`os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/dir.rs`：

- `sys_mkdirat` 与 `sys_mknodat` 在 `resolve_path_at` 后追加
  `resolve_symlinks(..., FinalSymlink::NoFollow)`。
- 中间符号链接会按路径解析规则展开，超过 40 层时返回 `ELOOP`；最终链接不跟随，
  保持创建语义。

## 验证

```text
make check ARCH=rv PROFILE=final
make check ARCH=la PROFILE=final
```

LTP 定向日志 `/tmp/mkdirat-eloop-fixed.log`：

```text
mkdirat02: EROFS x2 / ELOOP x2 全部 TPASS
FAIL LTP CASE mkdirat02 : 0
```

## 后续

`pathconf02` 的 `EACCES` 仍失败，说明 glibc `pathconf` 没有经过本次已修的
`stat/statx` 权限路径，需要单独跟踪其底层 syscall。

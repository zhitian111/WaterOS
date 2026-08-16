# Task 01 诊断补充简报

## 完成情况

在 `bringup-stats` 中增加 `statx`、`openat`、`readlinkat` 计数，默认关闭，仅
`bringup-stats` feature 下生效。用于判断单核编译阶段路径/元数据 syscall 的量级。

## 改动文件

- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/misc/bringup_stats.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/fstat.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/openat.rs`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/dir.rs`

## 验收

```text
HEAP_ALLOCATOR_FEATURE=heap-slab make rv_check   PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make la_check   PASS
git diff --check                                  PASS
```


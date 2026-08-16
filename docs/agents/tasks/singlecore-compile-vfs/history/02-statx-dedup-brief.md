# Task 02 简报：statx 去除重复路径分量校验

## 完成情况

`sys_statx` 原先先做一次 `reject_long_path_component`，随后 `resolve_path_at`
内部又调用 `validate_path_components`。移除 syscall 层重复校验，保留 resolve 层校验。

## 改动文件

- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/fs/fstat.rs`

## 验收

```text
HEAP_ALLOCATOR_FEATURE=heap-slab make rv_check   PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make la_check   PASS
git diff --check                                  PASS
```


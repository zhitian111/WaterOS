# Task 05：futex / clock_gettime 无竞争快路径

## 任务目标

降低单核编译中同步和计时 syscall 的固定开销。

## 实施方案

1. futex 无竞争时先原子比较，不进入 waitqueue。
2. `clock_gettime` 减少重复时间换算和 CSR 读取。
3. 短 `nanosleep` 保留轻量忙等阈值，避免无意义调度。

## 涉及文件

- `os/components/wateros-ipc/ipc-futex/**`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/time/**`
- `os/components/wateros-syscall/syscall-impl/impl-kernel/src/sys/ipc/futex.rs`

## CodeGraph 查询

```bash
cd /tmp/wateros-singlecore-compile-vfs
codegraph explore "sys_futex sys_clock_gettime sys_nanosleep"
codegraph impact "futex_wait"
```

## 验收方式

```bash
cd /tmp/wateros-singlecore-compile-vfs/os
HEAP_ALLOCATOR_FEATURE=heap-slab make rv_check
HEAP_ALLOCATOR_FEATURE=heap-slab make la_check
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-rv-final
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-la-final
git diff --check
```

运行时 RV/LA 各 1 轮 BuildStorm，确认无 futex 丢唤醒/超时异常。

## 完成后

新增 `history/05-brief.md`。


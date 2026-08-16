# Task 03：paged_handle 顺序读预取

## 任务目标

对单核 rustc 大量读取的源文件、rlib 和目标文件做低风险顺序 readahead。

## 实施方案

1. 在 `paged_handle.read_at` 检测顺序读：

   ```text
   next_offset == last_read_end
   ```

2. 命中连续模式时预取下一到两个 page。
3. 预取只进 page cache，不阻塞当前 syscall。
4. 对随机读不触发额外 I/O。

## 涉及文件

- `os/components/wateros-vfs/vfs-impl/impl-fs-bridge/src/paged_handle.rs`
- `os/components/wateros-vfs/vfs-impl/impl-page-cache/src/**`

## CodeGraph 查询

```bash
cd /tmp/wateros-singlecore-compile-vfs
codegraph explore "PagedFileHandle read_at PageCacheIo"
codegraph impact "read_at"
codegraph callers "read_at"
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

运行时 RV/LA 各 1 轮 BuildStorm，确认读结果正确且无明显内存增长。

## 完成后

新增 `history/03-brief.md`。


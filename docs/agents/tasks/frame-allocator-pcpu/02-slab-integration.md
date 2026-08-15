# Task 02：slab 补页改走 per-CPU frame batch

## 任务目标

把 slab 的小对象页补充从直接调用全局 `alloc_frame` 改为当前 CPU 的 frame batch，
让对象快路径真正形成两级 per-CPU 缓存。

## 实施方案

1. `HeapFrameSourceAdapter` 或 slab frame source 增加当前 CPU batch 访问。
2. `SlabCache::alloc` 在需要新页时调用 batch 分配；失败时回退全局慢路径。
3. 保留现有 `partial` 页复用逻辑；空页仍可在 cache 内复用，优先不归还 batch。
4. 如实现 slab 页归还，归还到当前 CPU batch；batch 满再批量 drain 到全局。
5. 记录 batch 命中率和全局补充次数，供 Task 05 性能分析。

## 涉及文件

- `os/components/wateros-runtime/runtime-heap-allocator/src/slab/**`
- `os/components/wateros-mm/mm-frame-alloctor/frame-alloctor-impl/impl-stack/src/lib.rs`
- `os/src/main.rs`

## CodeGraph 查询

```bash
cd /tmp/wateros-frame-allocator-pcpu
codegraph explore "SlabCache::alloc HeapFrameSourceAdapter"
codegraph impact "HeapFrameSource"
codegraph callers "frame_alloc_result"
```

## 验收方式

```bash
cd /tmp/wateros-frame-allocator-pcpu/os
HEAP_ALLOCATOR_FEATURE=heap-slab make rv_check
HEAP_ALLOCATOR_FEATURE=heap-slab make la_check
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-rv-final
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-la-final
git diff --check
```

运行时：

- RV/LA 各至少 1 轮完整 BuildStorm；
- `frame_mem_stats`、`heap_slab_stats` 无异常；
- 无 OOM、无 frame allocator duplicate/invalid 告警。

## 完成后

新增 `history/02-brief.md`。


# Task 01 简报：frame allocator per-CPU batch API

## 完成情况

在 `StackFrameAllocator` 之上增加 per-CPU frame batch，实现 `frame_alloc_batch_result`
和 `frame_dealloc_batch_result`，batch 容量为 16 页。

## 关键实现

- 每个 CPU 一个 `FrameBatch` 槽位；
- 分配优先从 batch pop，空时一次持全局锁批量补充最多 16 页；
- 释放优先 push 到 batch，满时一次性批量归还 batch 和当前页；
- 连续帧分配路径不经过 batch，仍走全局 `alloc_contiguous`。

## 改动文件

- `os/components/wateros-mm/mm-frame-alloctor/frame-alloctor-impl/impl-stack/src/lib.rs`

## 验收

```text
HEAP_ALLOCATOR_FEATURE=heap-slab make rv_check   PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make la_check   PASS
git diff --check                                  PASS
```

## 未验证项

- 尚未接入 slab 和完整 BuildStorm。


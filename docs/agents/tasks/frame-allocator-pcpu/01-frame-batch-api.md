# Task 01：frame allocator 增加 per-CPU batch refill/release 语义

## 任务目标

为全局 frame allocator 增加 per-CPU frame batch 快路径：本 CPU 优先从 batch 取页/
还页，只有 batch 空/满时才进入全局慢路径。

## 实施方案

1. 设计 `FrameBatchCache`，每个 CPU 一个槽位，容量取小批量，例如 8/16/32 页。
2. 增加批量操作：

   - `refill(cpu, count)`：一次持有全局 frame allocator 锁取 `count` 页；
   - `drain(cpu, count)`：一次持锁归还多余页。

3. 保证所有权语义：

   - batch 内页在全局 allocator 中仍视为已分配；
   - 单页分配优先从当前 CPU batch 取；
   - 单页释放优先回当前 CPU batch；
   - 连续帧分配仍直接走全局慢路径，不经过 batch。

4. 更新 `frame_mem_stats` 时正确呈现 batch 占用。

## 涉及文件

- `os/components/wateros-mm/mm-frame-alloctor/frame-alloctor-impl/impl-stack/src/lib.rs`
- `os/components/wateros-mm/mm-frame-alloctor/frame-alloctor-api/api-v0/src/lib.rs`
- `os/src/main.rs` 的 `HeapFrameSourceAdapter`

## CodeGraph 查询

```bash
cd /tmp/wateros-frame-allocator-pcpu
codegraph explore "StackFrameAllocator alloc_frame dealloc_frame"
codegraph impact "with_frame_allocator"
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

运行时至少：

- RV/LA 各 1 轮 BuildStorm 功能通过；
- 不出现重复释放、`allocated` 状态错误、refcount 异常或 panic。

## 完成后

新增 `history/01-brief.md`。


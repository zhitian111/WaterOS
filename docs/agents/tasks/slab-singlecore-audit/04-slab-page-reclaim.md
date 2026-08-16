# Task 04：实现空 slab 页回收并修正内存统计

## 任务目标

消除小对象 slab 的永久物理页滞留，补齐 slab page/frame 统计，确保 OOM/high-water 诊断
反映真实资源。

## 实施方案

1. 设计明确的 empty/partial/current 状态转换；只有页上所有对象均释放且不在 remote 队列
   时才进入回收候选。
2. 回收前从本地 partial/current 链表摘除，调用 `HeapFrameSource::dealloc_frame`；RV
   使用 batch return，LA 使用其对应 frame API。
3. 设定每 class 至少保留一个 warm page，避免频繁 alloc/free 抖动；上限由诊断数据决定。
4. 使 `heap_mem_stats()`、slab page counters 和 frame stats 的容量/已用语义一致，更新
   OOM 日志和 README 契约。

## 验收方式

功能单测必须验证：分配多页、全部释放、frame source free count 增加；partial 页不会过早
回收；remote free 未 drain 前不会回收；再次分配可复用已回收页。替换当前无意义的
`free_count() >= 0` 断言。

```bash
cd os
make rv_check
make la_check
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-rv-final
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-la-final
git diff --check
```

运行短压力：重复分配/释放各 class 至少 10,000 次，检查无重复帧、UAF、OOM、递归分配和
frame count 泄漏。双架构静态/构建和定向功能门禁必须通过；RISC-V BuildStorm smoke 必须
通过。性能暂允许变化，但 RISC-V 超过 5%
回退需停止并分析回收阈值。

## 涉及文件与 CodeGraph

- `os/components/wateros-runtime/runtime-heap-allocator/src/slab/slab_cache.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/slab/slab_page.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/slab/mod.rs`
- `os/src/main.rs`
- `os/components/wateros-mm/mm-frame-alloctor/frame-alloctor-impl/impl-stack/src/lib.rs`

```bash
codegraph explore "SlabPageHeader is_empty dealloc_frame frame_dealloc_batch_result HeapMemStats"
codegraph callers "HeapFrameSource::dealloc_frame"
```

## 完成后

新增 `history/04-brief.md`，记录状态机、回收计数、压力结果、统计字段变化和镜像功能结果。

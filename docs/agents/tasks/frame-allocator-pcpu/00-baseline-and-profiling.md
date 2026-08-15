# Task 00：记录 slab 基线并加低开销统计

## 任务目标

确认本分支的起点是 slab 最终状态，并为后续优化增加足够定位瓶颈、又不污染热路径
的统计计数。

## 实施方案

1. 记录 `perf/kernel-heap-slab` 当前 RV/LA 同配置 BuildStorm 中位数作为基线。
2. 在 slab 路径增加低开销计数：

   - 每 size class 本地对象命中 / 页补充次数；
   - remote-free push / drain 次数；
   - 大对象回退 boot TLSF 次数；
   - frame batch cache 命中 / 全局补充次数（本任务先留接口）。

3. 计数使用 `AtomicUsize`，只做 `Relaxed`/`Acquire` 级别必要操作，默认不打印；
   通过现有 `heap_slab_stats` 或诊断接口读取。
4. 保持两架构构建通过，功能不受影响。

## 涉及文件

- `os/components/wateros-runtime/runtime-heap-allocator/src/slab/**`
- `os/components/wateros-runtime/runtime-heap-allocator/src/lib.rs`
- 可能涉及 `os/components/wateros-mm/mm-frame-alloctor/**`

## CodeGraph 查询

```bash
cd /tmp/wateros-frame-allocator-pcpu
codegraph explore "SlabAllocator SlabCache CpuSlab"
codegraph impact "alloc_frame"
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

- RV 8 核 / LA 12 核各完成 1 轮 BuildStorm；
- 统计计数不 panic、不引入明显性能回退。

## 完成后

新增 `history/00-brief.md`。


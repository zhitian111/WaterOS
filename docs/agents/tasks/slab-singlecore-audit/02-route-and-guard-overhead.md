# Task 02：消除 slab 路由重复计算与 fallback guard 额外成本

## 任务目标

缩短 slab 前端对每次分配的固定链路：避免重复 `SizeClass::from_layout`，并避免大对象或
slab miss 先进入一层外部 interrupt guard、随后又进入 TLSF guard。

## 实施方案

1. 在 `GlobalAlloc` 入口只计算一次路由结果，将 `SizeClass`/class index 传入 slab backend。
2. 对确定不会走 slab 的 layout，在进入 guard 前直接调用 TLSF backend；大对象连续帧路径
   也只使用一层 guard。
3. 保持 `GlobalAlloc` 递归检测、NULL/zero realloc、跨后端 dealloc 和 layout 对齐语义。
4. 不在本任务重写 remote-free 或页回收，便于独立归因。

## 验收方式

```bash
cd os
make rv_check
make la_check
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-rv-final
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-la-final
git diff --check
```

添加/运行边界测试：0 字节、2048/2049 字节、对齐 2048/4096、跨 class realloc、boot TLSF
指针 dealloc、slab 指针跨 CPU dealloc。必须无 panic、double free、数据破坏或错误 OOM。

反汇编检查 guard 调用和 `SizeClass::from_layout` 调用数量；性能用任务 00 的镜像和
`-snapshot` 做 TLSF/slab A/B/B/A。若 RV 或 LA 中位数退化超过 2%，回退本任务。

## 涉及文件与 CodeGraph

- `os/components/wateros-runtime/runtime-heap-allocator/src/lib.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/slab/mod.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/backend_tlsf.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/interrupt_guard.rs`

```bash
codegraph explore "KernelAllocator GlobalAlloc alloc dealloc realloc SizeClass::from_layout with_allocator_interrupt_guard"
codegraph callers "fits_existing_class"
```

## 完成后

新增 `history/02-brief.md`，记录每个路由的 guard 数量、边界测试和双架构 A/B 结果。


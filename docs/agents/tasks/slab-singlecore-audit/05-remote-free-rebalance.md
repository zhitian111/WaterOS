# Task 05：重做 remote-free 的有界 drain 与页面再平衡

## 任务目标

解决 owner CPU 固定绑定、单一无界 remote 队列和跨 CPU 对象滞留问题，使迁移频繁的
BuildStorm 任务不会把 slab 内存分割到不可复用的 CPU 上。

## 实施方案

1. 先使用任务 03 数据决定采用哪一种窄改：按 owner+class 分队列并限制单次 drain，或将
   remote free 发布到页级并发 freelist/central partial list。
2. drain 必须有上限，不能在一次分配中完整扫描任意长度的队列；剩余队列由后续分配或
   明确的后台回收点继续处理。
3. 支持空闲 owner 的页转移或 central partial 重新归属；保留 ABA、页存活和对象不重复
   入队不变量。
4. 远程释放路径不得依赖 allocator heap 分配、日志或可阻塞锁。

## 验收方式

功能压力至少覆盖：CPU 0 分配/CPU 1 释放、owner 不再分配、多个 class 混合远程释放、
并发 CAS 失败、队列长度超过 drain 上限、页回收和再次跨 CPU 使用。检查每个对象恰好
出现一次、无 UAF/double free/丢失对象。

```bash
cd os
make rv_check
make la_check
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-rv-final
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-la-final
git diff --check
```

使用任务 03 诊断比较 remote 比例、最大 drain 长度、page high-water；再做 RISC-V 完整
BuildStorm A/B。任何功能错误立即回退；性能至少不能比任务 04 候选再退化 2%。

## 涉及文件与 CodeGraph

- `os/components/wateros-runtime/runtime-heap-allocator/src/slab/cpu_slab.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/slab/slab_cache.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/slab/slab_page.rs`
- 需要时 `os/components/wateros-base/src/cpu.rs`

```bash
codegraph explore "CpuSlab remote_push drain_remote SlabCache dealloc_local SlabPageHeader owner_cpu"
codegraph callers "remote_push"
```

## 完成后

新增 `history/05-brief.md`，记录选择的再平衡方案、不变量、压力结果和 RISC-V 性能结果。

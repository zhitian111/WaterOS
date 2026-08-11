# 2026-08-08：TLSF per-CPU 小对象缓存实验（已回退）

## 思路

pc-hot 中 TLSF allocate/deallocate 仍在 Top 10。尝试给 `runtime-heap-allocator`
增加 per-CPU 小对象缓存：≤ 1024 字节、对齐 ≤ 16 的重复分配先走本核缓存，缓存满再
回全局 TLSF，减少全局 `spin::Mutex` 竞争。

## 结果

同一 200s Final 早期窗口，与 mmap 搜索跳转后的基线对比：

| 指标 | mmap 跳转基线 | TLSF 小对象缓存（256B） | TLSF 小对象缓存（1KiB） |
|---|---:|---:|---:|
| `Tlsf::allocate` | 1.22B | 1.18B | 1.14B |
| allocator guard alloc 路径 | 0.55B | 1.06B | 1.41B |
| `Tlsf::deallocate` | 0.86B | 0.83B | 0.80B |

## 结论

缓存命中没有带来可复现收益，反而把 per-alloc 的 guard 路径显著变重；分配器是
全内核最高风险组件，不能在没有收益时保留。该实验已从工作区回退。

## 后续

- 先统计 BuildStorm 分配 size 分布，确认热点是否真的是小对象。
- 若需要继续，优先考虑减少高频短生命周期对象，而不是继续扩大 per-CPU 缓存。
- 也可以直接针对 `with_allocator_interrupt_guard` 内的统计/高水位开销做更小改动。

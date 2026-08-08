# TLSF 懒统计 A/B（已回退，2026-08-09）

## 实验内容

`InterruptSafeTlsfHeap::alloc` 原本每次分配都读取 `mem_stats()` 的两个原子字段，
再交给高水位告警函数。实验改为：只有高水位告警尚未触发时才读取统计，减少每次
alloc 的热路径原子负载。

## pc-hot A/B（同 180s Final 早期窗口）

```text
基线: /tmp/pcs-rv-current-20260809b.txt
当前: /tmp/pcs-rv-tlsf-stats-lazy-20260809.txt
日志: /tmp/pc-hot-tlsf-stats-lazy-20260809.log
```

| 指标 | 基线 | 当前 |
|---|---:|---:|
| 总指令 | 22.94B | 24.16B |
| TLSF `allocate` | 1.056B | 1.123B |
| alloc guard 路径 | 476M | 560M |

## 结论

懒统计没有降低 TLSF 或 allocator guard 指令，总指令反而上升约 5.3%。没有净收益，
已回退，不进入完整 Final。

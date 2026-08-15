# Task 09 简报：slab rebase VMA 后重新验证与修复

## 完成情况

`perf/kernel-heap-slab` 已 rebase 到 VMA 分支 HEAD
`fffad37f97d6e0c72efb992756ab2f1222990139`，并修复 rebase 后暴露的 slab
运行时问题。

## 关键修复

- 将 Sv39 默认 feature 恢复为 `elf-lazy-map`，去掉旧 workaround；
- 修正 `SlabPageHeader` 实际大小：`SLAB_HEADER_SIZE` 从 64 改为 72，
  并在 `slab_page.rs` 中增加编译期断言；
- 修正 size class 对齐计算：2 次幂 class 使用自身大小对齐，非 2 次幂
  class 使用小于 size 的最大 2 次幂对齐；
- 将 remote-free 队列从 `CpuSlab` 中拆出，避免 owner CPU 可变访问本地
  cache 时与 remote push 产生 Rust 别名/UB 风险。

## 静态验收

```text
HEAP_ALLOCATOR_FEATURE=heap-slab make rv_check   PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make la_check   PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-rv-final   PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-la-final   PASS
git diff --check                                  PASS
```

## 运行时验收

| 架构 | 配置 | 结果 | elapsed_s | 日志 |
|:--|:--|:--|:--|:--|
| RV | 8 核，16G | `status=OK run=OK` | 565.23 | `/tmp/wateros-slab-rv-smp8-16g-fix5.log` |
| LA | 12 核，16G | `status=OK run=OK` | 548.76 | `/tmp/wateros-slab-la-smp12-16g.log` |

日志 SHA-256：

```text
7af2f070d9fa6943f853434fa4a58203e6905f9fdd70e0b3298f2142813f5374  /tmp/wateros-slab-rv-smp8-16g-fix5.log
243aa7ca7a73201e7346e92f80f94992a43b24f3621f9c416a8c8ac37df2db14  /tmp/wateros-slab-la-smp12-16g.log
```

## 剩余事项

- 本次因宿主机 swap 压力，LA 性能验证使用 16G，而非用户原定 36G；
- 后续在内存充足时补跑 36G 最终性能；
- 继续分析 slab 相对 VMA/baseline 的性能差异，决定是否需要进一步调参。

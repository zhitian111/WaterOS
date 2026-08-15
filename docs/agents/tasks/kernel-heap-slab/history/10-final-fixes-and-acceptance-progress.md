# Task 10 简报：rebase 后 slab 修复与同配置验收进展

## 完成情况

在 `perf/kernel-heap-slab` rebase 到 VMA 分支后，修复了导致 RV 8 核 BuildStorm
偶发 SIGSEGV 的 TLB 旧权限/旧 PPN 问题，并补齐 slab 对齐与热路径优化。

## 关键提交

```text
59be6bbc [fix] 用户缺页在页表已满足访问时刷新本地 TLB
a7731a29 [heap-slab] 修正 size class 对象对齐并恢复 remote-free 空队列快路径
4f7514be [perf] 将 paged_handle seek 热路径日志降为 trace
```

## 静态验收

```text
HEAP_ALLOCATOR_FEATURE=heap-slab make rv_check   PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make la_check   PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-rv-final   PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-la-final   PASS
git diff --check                                  PASS
```

## slab 16G 完整 BuildStorm

### RV 8 核

| 轮次 | elapsed_s | 结果 |
|---|---:|---|
| 1 | 560.09 | OK |
| 2 | 565.32 | OK |
| 3 | 576.46 | OK |

中位数：`565.32`

### LA 12 核

| 轮次 | elapsed_s | 结果 |
|---|---:|---|
| 1 | 539.30 | OK |
| 2 | 549.09 | OK |
| 3 | 538.07 | OK |

中位数：`539.30`

## 同配置 VMA 对照

以下为共享修复后的 VMA 分支单轮对照，尚未补满 3 轮基线：

```text
RV 16G: /tmp/wateros-vma-rv-smp8-16g-sharedfix.log
  BUILDSTORM_RESULT ... elapsed_s=574.60 run=OK

LA 16G: /tmp/wateros-vma-la-smp12-16g-sharedfix.log
  BUILDSTORM_RESULT ... elapsed_s=547.98 run=OK
```

单轮比较：

```text
RV: 565.32 < 574.60
LA: 539.30 < 547.98
```

## 剩余事项

- VMA 基线仍应补足 RV/LA 各 3 轮中位数后，再作为最终 merge 依据；
- 当前证据已显示同配置下单轮 slab 优于 VMA，但不替代 3 轮中位数验收。

## 日志

```text
/tmp/wateros-slab-rv-smp8-16g-tlb.log
/tmp/wateros-slab-rv-smp8-16g-tlb-r2.log
/tmp/wateros-slab-rv-smp8-16g-tlb-r3.log
/tmp/wateros-slab-la-smp12-16g-tlb.log
/tmp/wateros-slab-la-smp12-16g-tlb-r2.log
/tmp/wateros-slab-la-smp12-16g-tlb-r3.log
```

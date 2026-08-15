# Task 06 简报：BuildStorm 串行阶段首个优化

## 背景

BuildStorm 的 syscall 画像显示 `mprotect` 是最高频 syscall，且大量为 4 KiB
`PROT_READ|PROT_WRITE`。这些调用通常作用于私有匿名 lazy VMA 的同一权限区间。

## 完成情况

为 `LazyVmaSet` 增加 `has_different_perm`，两架构 `mprotect` 在 lazy VMA
权限未变化时跳过 `protect_lazy_file_vmas` 的拆分/重建，从而减少单核 rustc/allocator
串行路径上的 Vec 分配和 VMA 元数据扰动。

## 改动文件

- `os/components/wateros-mm/mm-impl/common/src/vma.rs`
- `os/components/wateros-mm/mm-impl/impl-sv39/src/user_heap_mmap.rs`
- `os/components/wateros-mm/mm-impl/impl-loongarch64/src/user_heap_mmap.rs`

## 验收

```text
HEAP_ALLOCATOR_FEATURE=heap-slab make rv_check   PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make la_check   PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-rv-final   PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make kernel-la-final   PASS
git diff --check                                  PASS
```

## 首轮 RV BuildStorm

```text
/tmp/wateros-frame-pcpu-rv-smp8-16g-mprotect-r1.log
BUILDSTORM_RESULT ... elapsed_s=552.05 run=OK
```

对照：

```text
frame batch 首轮 RV: 561.80
slab RV 首轮:       560.09
slab RV 中位数:     565.32
```

首轮显示 `552.05`，优于 frame batch 和 slab 中位数；仍需多轮确认。

## 组合优化 RV 三轮

```text
/tmp/wateros-frame-pcpu-rv-smp8-16g-all-r1.log: 578.57
/tmp/wateros-frame-pcpu-rv-smp8-16g-all-r2.log: 553.75
/tmp/wateros-frame-pcpu-rv-smp8-16g-all-r3.log: 562.63
```

RV 中位数：`562.63`，低于 slab RV 中位数 `565.32`。

## 组合优化 LA 首轮

```text
/tmp/wateros-frame-pcpu-la-smp12-16g-all-r1.log: 537.87
```

低于 slab LA 中位数 `539.30`，仍需补足 LA 三轮中位数。

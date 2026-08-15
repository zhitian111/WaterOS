# Task 03：低风险热路径优化

## 任务目标

在不改变 Linux 语义的前提下，先做风险低、可独立回退的热路径优化。

## 候选优化

1. `SizeClass::from_layout` 改为静态查找表或位运算，消除线性扫描。
2. remote-free 使用批量 push/drain，降低跨核 CAS 频率。
3. `realloc` 同 class 快路径和 `dealloc` 指针归属判断保持并加强。
4. 若统计显示大对象 TLSF 回退占比明显，再单独立项，不在本任务盲改。

## 涉及文件

- `os/components/wateros-runtime/runtime-heap-allocator/src/slab/size_class.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/slab/cpu_slab.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/slab/mod.rs`

## CodeGraph 查询

```bash
cd /tmp/wateros-frame-allocator-pcpu
codegraph explore "SizeClass::from_layout CpuSlab::remote_push"
codegraph impact "SizeClass"
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

运行时至少 RV/LA 各 1 轮 BuildStorm，确认无功能回退。

## 完成后

新增 `history/03-brief.md`。


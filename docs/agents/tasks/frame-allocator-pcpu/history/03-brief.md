# Task 03 简报：SizeClass 查找表优化

## 完成情况

将 `SizeClass::from_layout` 从每次线性扫描 16 个 class 改为 `CLASS_INDEX_BY_NEED`
静态查找表。先按 `align_up(max(size,1), align)` 归一化请求，再 O(1) 查表。

## 改动文件

- `os/components/wateros-runtime/runtime-heap-allocator/src/slab/size_class.rs`

## 验收

```text
HEAP_ALLOCATOR_FEATURE=heap-slab make rv_check   PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make la_check   PASS
git diff --check                                  PASS
```

## 未验证项

- 完整 BuildStorm 性能尚未单独 A/B。


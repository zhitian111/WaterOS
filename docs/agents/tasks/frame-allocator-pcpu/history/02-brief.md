# Task 02 简报：slab 补页接入 per-CPU frame batch

## 完成情况

`HeapFrameSourceAdapter` 的 `alloc_frame` / `dealloc_frame` 改为调用 batch 接口，
使 slab 小对象页补充优先走 per-CPU batch。

## 改动文件

- `os/src/main.rs`

## 验收

```text
HEAP_ALLOCATOR_FEATURE=heap-slab make rv_check   PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make la_check   PASS
git diff --check                                  PASS
```

## 未验证项

- 完整 BuildStorm 功能与性能尚未运行。


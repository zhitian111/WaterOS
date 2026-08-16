# Task 02 简报：VFS 路径解析钩子去锁

## 完成情况

`resolve_open_path` 原先每次调用都锁一个全局 `spin::Mutex<Option<fn>>`。改为
`AtomicPtr` 存储函数指针，启动时注册一次，之后热路径只做一次 Acquire load。

## 改动文件

- `os/components/wateros-vfs/vfs-api/api-v0/src/resolve.rs`

## 验收

```text
HEAP_ALLOCATOR_FEATURE=heap-slab make rv_check   PASS
HEAP_ALLOCATOR_FEATURE=heap-slab make la_check   PASS
git diff --check                                  PASS
```


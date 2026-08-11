# TLSF dealloc 指针合法性校验

## 现象

完整 BuildStorm 偶发：

```text
recursive heap allocation detected
```

临时诊断显示递归发生在 TLSF `dealloc` guard 内。无效指针被交给 TLSF 后，可能破坏
块头或触发后续分配，导致 allocator guard 递归。

## 修改

`os/components/wateros-runtime/runtime-heap-allocator/src/backend_tlsf.rs`：

- `dealloc` 在进入 allocator guard 前检查 `ptr` 是否落在
  `[HEAP_SPACE, HEAP_SPACE + KERNEL_HEAP_SIZE)` 且 `[ptr, ptr+size)` 不越界。
- 无效指针直接 `warn` 并返回，不再交给 `rlsf::Tlsf::deallocate`。

这不会改变正常释放语义，只阻止越界/失效指针进入 allocator 元数据操作。

## 验证

- `make check ARCH=rv PROFILE=final` 通过。
- `make check ARCH=la PROFILE=final` 通过。

完整 BuildStorm 仍需要后续长测确认该护栏是否消除 allocator 递归 panic。

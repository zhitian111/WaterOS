# Task 05：大对象回退与 realloc 正确性

## 任务目标

把“小对象 slab、大对象 boot backend”的分界和 `realloc` 语义做完整，避免
`Vec`/`String`/大缓冲在扩容、缩小、跨 size class 或跨后端迁移时出现错误。

本任务保证正确性；是否把大对象进一步迁到 frame allocator 由 task 06 处理。

## 实施方案

1. 明确分界：

   - `SLAB_MAX` 以上的 allocation 走 boot TLSF；
   - 对齐超过 slab 支持范围的对象也走 boot TLSF；
   - `Layout::size() == 0`、非法 `Layout` 按 `GlobalAlloc` 契约处理。

2. `realloc` 分四种情况：

   - null 指针：等同 alloc；
   - 新 size 为 0：dealloc 旧对象并返回 null；
   - 新旧布局都在 boot backend：沿用 TLSF reallocate 或等价 copy；
   - 跨 slab/boot 或跨 size class：alloc 新块 → copy min 字节 → dealloc 旧块。

3. `dealloc` 路径必须统一判断来源：

   ```text
   slab-page header valid -> slab dealloc
   ptr in HEAP_SPACE range -> boot TLSF dealloc
   otherwise -> 记录并忽略/panic，按现有 diagnostics feature 策略
   ```

4. 增加针对大对象和 realloc 的 crate 测试。

## 涉及文件

- `os/components/wateros-runtime/runtime-heap-allocator/src/lib.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/backend_tlsf.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/slab/**`
- 相关测试模块

## CodeGraph 查询

```bash
cd /tmp/wateros-kernel-heap-slab
codegraph explore "GlobalAlloc realloc dealloc_pointer_in_heap"
codegraph impact "InterruptSafeTlsfHeap"
codegraph explore "SlabPage size_class"
```

## 验收方式

```bash
cd /tmp/wateros-kernel-heap-slab/os/components/wateros-runtime
cargo test -p wateros-runtime-heap-allocator --lib
cd /tmp/wateros-kernel-heap-slab/os
make rv_check
make la_check
make kernel-rv-final
make kernel-la-final
git diff --check
```

测试必须覆盖：

- 大对象 alloc/dealloc；
- 大对象 realloc 扩容/缩容；
- 小对象扩到大对象、大对象缩到小对象；
- `Vec` 多轮 push/pop/capacity 变化；
- 0 大小和非法 layout 边界。

运行时：

- 两架构至少各 1 轮完整 buildstorm；
- 重点观察 `Vec`/cargo/rustc 路径下无 `realloc` 相关 panic 或内存破坏。

## 完成后

新增 `history/05-brief.md`，记录 realloc 策略、测试覆盖和运行结果。

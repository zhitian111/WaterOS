# Task 06：大对象迁入 frame allocator 的连续帧路径

## 任务目标

在 task 05 正确性的基础上，为超过单页的大对象提供连续物理帧分配，减少大对象对
全局 boot TLSF 锁的依赖。若性能数据证明大对象不是热点，也要完成该能力并保留
feature/统计开关，不能留下未实现路径。

## 实施方案

1. 在 frame allocator 侧增加“连续帧分配”能力：

   - 建议新增 `frame_alloc_contiguous_result(pages)` /
     `frame_dealloc_contiguous_result(frame, pages)`；
   - 首选在 `impl-stack` 的 `next_novel` 连续高段上实现降序连续分配；
   - 若回收栈导致连续性不足，先显式返回错误，由 slab 回退到 boot TLSF。

2. 在 `runtime-heap-allocator` 中扩展 `HeapFrameSource`：

   ```rust
   fn alloc_contiguous(&self, pages: usize) -> Option<HeapFrame>;
   fn dealloc_contiguous(&self, frame: HeapFrame, pages: usize);
   ```

   单页路径仍用 `alloc_frame/dealloc_frame`。

3. 大对象路由：

   - 单页大对象：直接 `alloc_contiguous(1)`；
   - 多页大对象：按 `layout.size` 对齐到页，调用 `alloc_contiguous`；
   - 分配失败时允许回退 boot TLSF，但要记录统计，不能静默吞掉 OOM。

4. 大对象释放必须知道页数；用页首 header 或 page metadata 记录 order/pages。

## 涉及文件

- `os/components/wateros-mm/mm-frame-alloctor/frame-alloctor-impl/impl-stack/src/lib.rs`
- `os/components/wateros-mm/mm-frame-alloctor/src/lib.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/slab/**`
- `os/components/wateros-runtime/runtime-heap-allocator/src/lib.rs`
- `os/src/main.rs` 或 frame source 适配文件

## CodeGraph 查询

```bash
cd /tmp/wateros-kernel-heap-slab
codegraph explore "StackFrameAllocator next_novel frame_alloc_result"
codegraph impact "frame_alloc_result"
codegraph callers "frame_dealloc_result"
codegraph explore "HeapFrameSource large_alloc"
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

必须验证：

- 连续帧分配在空闲段不足/回收段不连续时返回错误；
- 大对象 alloc/dealloc/realloc 不再依赖 boot TLSF；
- 页数记录与归还数量一致；
- 两架构完整 buildstorm 仍通过。

所有 QEMU 运行均加 `-snapshot`。

## 完成后

新增 `history/06-brief.md`，记录连续帧接口、失败回退策略、大对象覆盖率和性能变化。

# Task 01 简报：GlobalAlloc 后端抽象

## 完成情况

完成。`#[global_allocator]` 现在由无状态的 `KernelAllocator` 门面持有，按编译期
feature 委托给唯一活动后端；TLSF 与链表后端实现统一 `HeapBackend` 接口。默认行为
未改变。

## 改动文件

- `os/components/wateros-runtime/runtime-heap-allocator/src/heap_backend.rs`（新增）
- `os/components/wateros-runtime/runtime-heap-allocator/src/lib.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/backend_tlsf.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/backend_linked_list.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/README.md`

## 关键设计

- `HeapBackend`：`init` / `mem_stats` / `alloc` / `dealloc` / `realloc`；
- `KernelAllocator`：`GlobalAlloc` 门面，委托 `backend::ACTIVE_ALLOCATOR`；
- 两个后端静态对象均去掉 `#[global_allocator]`，改为 `ACTIVE_ALLOCATOR`；
- 后续 slab 后端实现同一 trait，再由 Task 03 增加运行期切换。

## 验收命令与结果

```text
make rv_check                    PASS（仅有既有 warning）
make la_check                    PASS（仅有既有 warning）
make kernel-rv-final             PASS
make kernel-la-final             PASS
HEAP_ALLOCATOR_FEATURE=heap-linked-list make kernel-rv-final  PASS
git diff --check                 PASS
```

## 未验证项/风险

- 未做运行期 QEMU 功能回归（本任务不改变默认后端行为）；
- Task 03 才引入运行期 boot/slab 切换，`KernelAllocator` 当前仍是编译期选择。

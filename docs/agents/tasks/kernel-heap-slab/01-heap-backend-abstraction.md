# Task 01：抽象 GlobalAlloc 后端选择

## 任务目标

在不改变默认运行行为的前提下，把当前 `runtime-heap-allocator` 里直接依赖
TLSF/链表后端和 `HEAP_SPACE` 的 `#[global_allocator]` 抽象为一个可扩展的
`HeapBackend` 门面，为后续接入 frame-backed slab 做准备。

本任务完成后，默认 feature 仍选择 TLSF，现有内核启动和分配路径不能出现行为变化。

## 实施方案

1. 新增后端抽象层，至少表达以下能力：

   - `init(&self)`：初始化后端；
   - `alloc/dealloc/realloc`：对应 `GlobalAlloc` 语义；
   - `mem_stats(&self)`：返回 `HeapMemStats`；
   - `backend_kind()`：用于诊断日志区分后端。

2. 将现有 TLSF 和 linked-list 实现改为该抽象的实现：

   - `backend_tlsf.rs`：保持 `InterruptSafeTlsfHeap` 和 `HEAP_SPACE` 逻辑不变；
   - `backend_linked_list.rs`：保持 `InterruptSafeLockedHeap` 逻辑不变。

3. 在 `runtime-heap-allocator` 中按 feature 选择活动后端，并让公开接口
   `init/alloc_error/stats` 只依赖门面，不再直接 `use backend`。

4. 保留顶层 feature 语义：

   - `heap-tlsf` 仍是默认；
   - `heap-linked-list` 仍可切回链表；
   - 两个 feature 同时开启仍触发 `compile_error!`。

## 涉及文件

- `os/components/wateros-runtime/runtime-heap-allocator/src/lib.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/backend_tlsf.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/src/backend_linked_list.rs`
- 可能新增 `os/components/wateros-runtime/runtime-heap-allocator/src/backend.rs`
- `os/components/wateros-runtime/runtime-heap-allocator/Cargo.toml`
- `os/components/wateros-runtime/Cargo.toml` 仅当 feature 传播链需要同步时改动

## CodeGraph 查询

```bash
cd /tmp/wateros-kernel-heap-slab
codegraph explore "InterruptSafeTlsfHeap InterruptSafeLockedHeap HEAP_ALLOCATOR"
codegraph impact "HEAP_ALLOCATOR"
codegraph callers "heap_allocator::init"
codegraph callees "heap_allocator::handle_alloc_error"
```

## 验收方式

```bash
cd /tmp/wateros-kernel-heap-slab/os
make rv_check
make la_check
make kernel-rv-final
make kernel-la-final
git diff --check
```

附加检查：

- 默认 `make kernel-rv-final` 仍构建 TLSF 后端；
- `HEAP_ALLOCATOR_FEATURE=heap-linked-list make kernel-rv-final` 仍可构建链表后端；
- `rg -n "HEAP_ALLOCATOR|backend::" components/wateros-runtime/runtime-heap-allocator/src` 确认无遗漏直接引用；
- `make rv_check`/`make la_check` 均无新增 error。

功能层面本任务不改变运行语义；若条件允许，用现有镜像做一次 `make rv_final_run` 或
`make la_final_run` 冒烟。

## 完成后

新增 `history/01-brief.md`，记录抽象后的模块结构、检查结果和 feature 传播变更。

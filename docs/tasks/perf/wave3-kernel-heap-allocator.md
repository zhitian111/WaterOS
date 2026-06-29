# 性能任务：内核堆分配器替换（D1 fork/exit 骤降）

## 任务目标

解决 **fork/exit 大量执行后两者同时骤降**：将 `linked_list_allocator::LockedHeap`（first-fit O(空洞数)）替换为 **有上界复杂度** 的分配器，使 alloc/dealloc 延迟不随碎片单调恶化。

## 背景（必读）

- `docs/todo/perf-fork-exit-degradation.md` §D1（主因）
- 当前：`os/components/wateros-runtime/runtime-heap-allocator/src/lib.rs`

## 执行前必须参考的 prompt

- `docs/prompts/general.md`
- `docs/prompts/structure.md`
- `docs/prompts/coding.md`
- `docs/prompts/architecture.md`

## 需要优先查看的源文件

| 文件 | 用途 |
|------|------|
| `os/components/wateros-runtime/runtime-heap-allocator/src/lib.rs` | 全局 `HEAP_ALLOCATOR` |
| `os/components/wateros-runtime/runtime-heap-allocator/Cargo.toml` | 依赖 |
| `os/components/wateros-base/base-config/**` | `KERNEL_HEAP_SIZE` |
| `docs/todo/perf-risk-assessment.md` | 历史 buddy 与 UAF 权衡 |

## 方案选型（实施前与用户确认其一）

| 方案 | 复杂度 | 说明 |
|------|--------|------|
| **A. TLSF（rlsf 等）** | O(1) | 推荐，抗碎片 |
| **B. buddy_system_allocator** | O(log n) | 项目曾用；须单独保证无 UAF 破坏元数据 |
| **C. slab + 全局堆** | 混合 | TCB/PCB 等固定大小走 slab |

## 实施要点

1. 用 **Cargo feature** 保留旧分配器路径，便于回退（`impl-linked-list-allocator` vs `impl-tlsf` 等）。
2. 保持 `InterruptSafeLockedHeap` 关中断包装与 `heap_mem_stats()` API。
3. 不改 `kernel_heap` 链接符号与 `KERNEL_HEAP_SIZE` 除非必要。
4. 提供或运行 **fork+exit 压测**：循环数万次，打印 `heap_mem_stats()`，验证时延不随轮次上升。

## 验收标准

- [ ] `make rv_check && make la_check`
- [ ] bringup busybox + LTP 抽样无 OOM/双重 free panic
- [ ] fork+exit 压测：后期轮次时延不明显高于前期（定性即可）
- [ ] feature 可切换回旧分配器

## 风险

- **中**：新 crate `no_std` 兼容性、realloc 行为差异

## 示例：交给 Agent 的一次性用户 prompt

```
@docs/tasks/perf/wave3-kernel-heap-allocator.md

请用 rlsf（TLSF）替换 linked_list_allocator，保留 feature 回退旧路径。
make rv_check && la_check，写简单 fork+exit 循环验证时延不恶化。
```

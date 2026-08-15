# Task 05 简报：大对象回退与 realloc 正确性

## 完成情况

完成。`KernelAllocator` 现在按 layout 路由：

- `size <= 2048 && align <= 2048` → slab；
- 否则 → boot TLSF；
- `realloc` 统一为 allocate-new → copy-min → dealloc-old，覆盖 null/0/跨后端/跨
  size class 情况；
- `dealloc` 以 boot `HEAP_SPACE` 地址范围区分来源，避免误路由。

## 验收依据

本任务没有新增独立 feature，改动随 Task 03/04 已进入内核；两架构 check/build 通过，
且 LA `-smp 12` 与 RV 单核完整 buildstorm（含大量 `Vec`/cargo/rustc realloc 压力）
均通过，未出现 realloc 相关 panic 或内存破坏。

## 未验证项

- host 单测仍受 arch asm 限制，realloc 边界依赖运行时 buildstorm 覆盖；
- 大对象连续帧路径（完全脱离 boot TLSF）留到 Task 06。

# runtime-heap-allocator

[项目首页](../../../../README.md) · [内核工程](../../../README.md) · [wateros-runtime](../README.md)

本 crate 提供 WaterOS 的 `#[global_allocator]`。默认 backend 为单 TLSF；也可选择固定静态
arena 的 `impl-per-cpu-tlsf` 或回退到 `impl-linked-list-allocator`，三个实现互斥。顶层以
`HEAP_ALLOCATOR_FEATURE=heap-per-cpu-tlsf` 选择 per-CPU 实现。

`HEAP_SPACE` 是链接脚本放入 `.kernel.heap (NOLOAD)` 的静态池，因此只占运行时地址空间，
不会以零填充撑大 ELF。`KERNEL_HEAP_SIZE` 当前为 256 MiB，在 per-CPU 实现中表示每个 arena
的容量：一个 early/global arena，加上 `MAX_CPUS` 个 CPU arena。只有 BSP 可调用一次
`init()`；核心初始化完成后由 `enable_per_cpu_arenas()` 切换新分配，早期对象仍按地址归还
global。AP 必须在切换发布后才可走可能分配的路径。

per-CPU 实现的 alloc 优先进入当前 CPU arena，本地 OOM 回退 global；dealloc/realloc 根据
指针地址回到 owner arena，因此支持跨 CPU 释放。每次操作仍在本 CPU 临时关中断并以
`CpuLocal` 深度检测递归，每个 arena 自己的锁只保护该 TLSF 元数据。

`heap_mem_stats()` 仅用于观测。TLSF 的 used 为 layout-size 估算，不是精确的可回收页数。
OOM handler 会记录布局和快照后 panic，不能尝试继续执行。

启用顶层 `heap-stress` feature 会在 BSP 初始化堆后运行固定的多尺寸分配/释放压力并
停机，只用于后端 A/B，不进入普通 pre/final 内核。

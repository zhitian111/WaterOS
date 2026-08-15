# runtime-heap-allocator

[项目首页](../../../../README.md) · [内核工程](../../../README.md) · [wateros-runtime](../README.md)

本 crate 提供 WaterOS 的 `#[global_allocator]`。默认 backend 为 TLSF，可用
`impl-linked-list-allocator` 切回 `LockedHeap`；二者互斥。项目构建可通过
`HEAP_ALLOCATOR_FEATURE=heap-linked-list` 选择回退实现。

`KernelAllocator` 是无状态全局分配器入口，编译期通过 `HeapBackend` 接口委托给
唯一活动后端（`backend_tlsf` / `backend_linked_list`）。后续 slab 后端也将实现
同一接口，由该入口按运行期状态切换。

`heap-slab` 顶层 feature 会启用 frame-backed 每核 slab；开启后 BSP 在 frame
allocator 初始化完成时通过 `register_frame_source` + `activate_slab` 切换，
小对象走 slab，大对象和早期启动分配仍走 boot TLSF。

开启 `heap-slab` 后可通过 `heap_slab_stats()` 读取 slab 分配/释放计数；
`heap_mem_stats()` 仍返回 boot TLSF 快照，slab 页占用统计在后续版本补齐。

`HEAP_SPACE` 是链接脚本放入 `.kernel.heap` 的静态池。只有 BSP 可调用一次 `init()`；
AP 必须在其后才可走可能分配的路径。每次分配在本 CPU 上临时关中断并以 `CpuLocal`
深度检测递归；backend 自身锁负责跨 CPU allocator 元数据互斥。

`heap_mem_stats()` 仅用于观测。TLSF 的 used 为 layout-size 估算，不是精确的可回收页数。
OOM handler 会记录布局和快照后 panic，不能尝试继续执行。

启用顶层 `heap-stress` feature 会在 BSP 初始化堆后运行固定的多尺寸分配/释放压力并
停机，只用于后端 A/B，不进入普通 pre/final 内核。

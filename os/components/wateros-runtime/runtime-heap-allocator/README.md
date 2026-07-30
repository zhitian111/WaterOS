# runtime-heap-allocator

本 crate 提供 WaterOS 的 `#[global_allocator]`。默认 backend 为 `LockedHeap`，可用
`impl-tlsf` 切换为 TLSF；二者互斥。

`HEAP_SPACE` 是链接脚本放入 `.kernel.heap` 的静态池。只有 BSP 可调用一次 `init()`；
AP 必须在其后才可走可能分配的路径。每次分配在本 CPU 上临时关中断并以 `CpuLocal`
深度检测递归；backend 自身锁负责跨 CPU allocator 元数据互斥。

`heap_mem_stats()` 仅用于观测。TLSF 的 used 为 layout-size 估算，不是精确的可回收页数。
OOM handler 会记录布局和快照后 panic，不能尝试继续执行。

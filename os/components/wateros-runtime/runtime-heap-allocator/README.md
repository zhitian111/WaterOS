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
小对象走 slab，大对象和早期启动分配仍走 boot TLSF。每个 CPU 的每个 size class 最多保留
一个 empty current 和一个 detached warm reserve，形成两页迟滞以避免阶段切换时反复
refill/return；其它页只有在全部对象已释放且 remote-free 队列已经由 owner CPU drain 后，
才从 current/partial 状态摘除并归还 frame allocator。RISC-V 使用 per-CPU batch 归还，
LoongArch64 直接归还全局 frame pool。

生产 `heap-slab` 不在 alloc/free 热路径维护全局统计。显式启用顶层
`slab-diagnostics` 后，allocator 才会编译 per-CPU、cache-line 对齐的统计槽；owner CPU
在关中断区内用原子 load/store 更新，bring-up 队列结束时一次性汇总 size class、local
hit/free、remote free/CAS miss/drain、frame refill/reclaim、当前页数、空页数、fallback 和
OOM。普通 Final 不包含这些诊断字段、更新调用或汇总符号。

`heap_mem_stats()` 把三个口径保持为独立字段：`used/free/capacity` 是 boot heap，
`slab_retained/slab_reclaimable` 是 slab 当前持有页和 detached warm reserve 的字节数，
`frame_used/frame_free/frame_capacity` 是全局 frame pool 快照。RISC-V 的 `frame_free` 不含
per-CPU frame batch 中缓存的页，因此这些字段不能简单相加推导系统总内存。诊断构建示例：

```bash
make build ARCH=rv PROFILE=final HEAP_ALLOCATOR_FEATURE=heap-slab \
  EXTRA_FEATURES=slab-diagnostics
```

`HEAP_SPACE` 是链接脚本放入 `.kernel.heap` 的静态池。只有 BSP 可调用一次 `init()`；
AP 必须在其后才可走可能分配的路径。每次分配在本 CPU 上临时关中断并以 `CpuLocal`
深度检测递归；backend 自身锁负责跨 CPU allocator 元数据互斥。

`heap_mem_stats()` 仅用于观测。TLSF 的 used 为 layout-size 估算；`slab_reclaimable` 不包含
仍作为 current 的空页，只表示 detached warm reserve，也不承诺调用后立即归还。OOM
handler 会分别记录 boot heap、slab 和 frame pool 快照后 panic，不能尝试继续执行。

启用顶层 `heap-stress` feature 会在 BSP 初始化堆后运行固定的多尺寸分配/释放压力并
停机，只用于后端 A/B，不进入普通 pre/final 内核。

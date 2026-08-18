# 物理帧分配器开发手册

[MM 总览](../README.md) · [API v0](frame-alloctor-api/api-v0/README.md) · [栈式实现](frame-alloctor-impl/impl-stack/README.md)

本组件管理普通 RAM 的 4 KiB 物理帧。它与 `runtime-heap-allocator` 是两套独立资源：前者供页表、用户页和页缓存使用，后者是内核 `Box`/`Vec` 等对象的固定堆。`/proc/meminfo` 的物理页充足并不能证明内核 heap 不会 OOM，反之亦然。

聚合层再导出 `api_v0` 和当前 `impl-stack`，并提供 `OwnedPhysPage`。后者是“一页、一个所有者、Drop 时回收”的 RAII 封装；它通过 `PPN * PAGE_SIZE` 访问内容，因此依赖内核对可分配 RAM 的恒等映射。

## 初始化与分配链

```text
kernel_mm::init
  -> 根据 kernel_end / RAM end / DTB 保留区计算 PPN 半开区间
  -> init_frame_allocator_with_reserved(...)
  -> 发布全局 allocator ready

页表或 fault
  -> frame_alloc_result / frame_alloc_zeroed_result
  -> 全局 allocator 锁
  -> recycled.pop()，否则从 next_novel 向下取页
  -> allocated=true, ref_count=1
```

回收时 `frame_dealloc_result` 实际减少引用计数；只有减至零才进入 `recycled`。页缓存、COW 或其它共享映射必须用 `frame_inc_ref_result` 建立额外引用，不能复制 `PhysPageNum` 后假定所有权自动复制。

## 零页池

后台零页池最多保留 1024 帧，低/高水位分别为 256/1024；同步补充批量 16，idle 批量 32，OOM 时最多排空 32。池中帧仍被主分配器标为 allocated 且引用计数为 1，不能同时出现在 `recycled`。`in_flight` 防止多个 CPU 预留后突破容量。

零页池只减少 fault 热路径清零成本，不增加物理内存。诊断物理 OOM 时应同时查看主帧统计与池内库存；仅比较 `free_frames` 可能误把池中可快速释放帧视为永久占用。

## 所有权检查表

- 普通匿名页、私有文件页和页表帧：最后一个引用负责回收到本分配器。
- 只读共享缓存页：cache 与每个 PTE 各持引用；任一安装失败都要撤销对应引用。
- 设备页：从不进入本分配器。
- SysV SHM 等外部页：由外部注册表持有；地址空间只删映射。
- `OwnedPhysPage`：不要把 `frame_id()` 误当转移所有权；借出的 slice 不能超过对象寿命。

## 诊断与回归

`FrameMemStats` 给出 `total_frames`、`free_frames` 和页大小。若 free 持续下降，按“创建地址空间 → fault → unmap/exit”分阶段采样，并检查页表帧、VMA resident 页和共享 cache 引用；若内核 heap 的 `used` 上升而物理 free 稳定，则检查 `Vec`/元数据或缓存，而不是扩 RAM。

回归至少包括：保留区永不返回、分配至 OOM、释放再复用、双重释放拒绝、引用计数、零页内容、并发分配唯一性、地址空间销毁后基线恢复。改变初始化区间时还要验证内核镜像、DTB 和 MMIO 没有被纳入帧池。


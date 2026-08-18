# VirtIO-MMIO Block 实现手册

[Block API](../../block-api/api-v0/README.md) · [Block 总览](../../README.md) · [RISC-V 机器探测](../../../driver-impl/impl-qemu-riscv64-virt/README.md)

本 crate 把一个已经由平台枚举出的 `virtio,mmio` 寄存器窗口包装成同步 `BlockDevice`。它不负责扫描 DTB、分区、缓存或文件系统，也没有中断完成队列；这些边界不可混用。

## 对外对象与调用链

唯一设备对象是 `VirtioBlkDevice`，内部持有：

```text
MmioRegion
  -> VirtioBlkDevice::from_mmio
  -> NonNull<VirtIOHeader>
  -> MmioTransport::new
  -> VirtIOBlk::new（feature negotiation + virtqueue）
  -> 平台包装 Arc<SpinMutex<dyn BlockDevice>>
  -> register_block_device
  -> VFS / block cache 调 read_blocks、write_blocks、flush
```

`MmioRegion.base` 为零时报 `InvalidDtb`；transport 头、magic、版本、设备类型或握手失败统一变成 `Unsupported`。调用构造器前必须完成 frame allocator 初始化，并保证整个 MMIO 窗口已在内核页表中按设备内存属性映射且永久有效。

`total_blocks()` 直接返回 VirtIO capacity。读写先调用 API 默认的 `check_request_range`，再把 `u64` LBA 转为 `usize`；越界、字节数非块整数倍或转换失败不得触碰设备。vendor I/O 错误统一映射为 `IoError`。`flush()` 是显式持久化屏障；文件系统不能用“写请求返回”替代它。

大于等于 4096 字节的写会输出 begin/end trace。通常调用者正持有设备 mutex，因此日志后端不得反向获取 block/VFS 锁，生产环境也应评估这一诊断对锁持有时间的影响。

## DMA HAL 的真实契约

`VirtioMmioHal` 是 `unsafe impl Hal`，正确性依赖以下平台事实：

- `PAGE_SIZE` 由编译期断言保证与 MM 的页大小相同；
- `dma_alloc(pages)` 逐页调用 `frame_alloc_result()`；
- 当前栈式 allocator 新鲜连续分配返回 `p, p-1, ...`，实现随后验证这个次序，取最后一个 PPN 为低地址；
- 成功区域被清零，返回 `paddr == vaddr`；
- `dma_dealloc` 从低 PPN 向上逐页归还；
- `share` 直接把普通内核指针解释为物理地址，`unshare` 为空。

这不是通用的连续页分配器。SMP 上若别的 CPU 插入一次 frame allocation，一次多页请求便会判定“不连续”、回收全部并失败；碎片化时也相同。若换成非恒等内核映射、IOMMU 或不可 DMA 的高端内存，`mmio_phys_to_virt`、`share` 和整个 alloc/dealloc 必须一起重写。

当前 `Vec<PhysPageNum>` 自身使用全局 heap，分配失败会走 Rust allocation error 而不是返回 `DriverError`。`pages * PAGE_SIZE` 清零长度也未 checked；vendor 正常队列规模不会触发，但 HAL 边界测试必须覆盖极值。

失败回滚：frame OOM、非连续 PPN、PPN 乘法溢出和空指针都会归还已经取得的页。`dma_dealloc` 忽略 frame allocator 的归还错误，因此 double-free/错误页数只可能从下层日志发现。

## 锁、生命周期与销毁

设备方法需要 `&mut self`，注册层通常用一个 spin mutex 串行化所有队列操作。持锁期间不得睡眠、等待另一个持同锁任务或回调 VFS。当前 vendor 接口同步推进队列，所以“方法返回”是 buffer 可再次使用的边界。

`MmioTransport<'static>` 的 `'static` 来源是裸 MMIO 指针，不代表硬件窗口由 Rust 拥有。平台映射必须活得比设备久。只有完整构造成功后才能注册；若 registry 永久持有 `Arc`，正常运行中设备不会 Drop。新增热拔插前必须先停止新 I/O、等待在途队列、从 registry 摘除，最后释放 transport DMA。

## 修改和扩展示例

新增 discard/write-zeroes 时：先在 Block API 增加能力查询和方法，再确认 vendor feature 位，构造器只在双方支持时公布能力。请求必须沿用 `check_request_range`，不能把 sector 与 WaterOS 的逻辑 block 混算。错误至少区分参数错误、未支持和设备 I/O。

若修复连续 DMA，优先给 frame allocator 增加原子的 `alloc_contiguous(pages, align)`/对称 free，而不是循环碰运气。成功结果应由一个 RAII owner 记录首 PPN 和页数，构造失败自动回收。

## 回归清单

- 零 base、过小窗口、错 magic/version/type、feature negotiation 失败；
- 0 页、1 页、多页、frame OOM、非连续注入和每条失败路径的 free-frame 基线；
- 首尾合法 LBA、末尾越界、非整块长度、`u64 -> usize` 溢出；
- 已知图样 write → flush → read，比对全部字节；
- 并发读写由 registry mutex 串行且无锁序反转；
- 反复构造/销毁后 frame 与 heap 基线恢复；
- 真实 QEMU 上跑文件系统一致性、iozone，并确认错误日志中无 queue/DMA 泄漏。

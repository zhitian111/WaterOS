# VirtIO-MMIO GPU 实现手册

[Display API](../../display-api/api-v0/README.md) · [RISC-V 探测](../../../driver-impl/impl-qemu-riscv64-virt/README.md)

本实现把 VirtIO-MMIO GPU 的 2D resource 暴露为 WaterOS framebuffer。它不是直接扫描输出，也没有硬件 cursor、多 display 或中断完成支持。

## 构造与数据结构

`VirtioGpuMmioDevice` 持有 vendor `VirtIOGpu` 和缓存的 `FramebufferInfo`。调用链为：

```text
平台得到 MmioRegion
  -> from_mmio
  -> MmioTransport::new
  -> VirtIOGpu::new
  -> resolution
  -> setup_framebuffer
  -> 计算 stride / byte_len / mapped_len / base
  -> 注册 DisplayDevice
```

当前像素格式固定为 `Bgra8888`，`stride = width * 4`，有效字节数为 `stride * height`，mapped length 向页上取整。构造必须拒绝算术溢出、vendor buffer 比有效字节短以及无效指针；修改分辨率路径时这些校验不能下放给用户态。

framebuffer slice 直接引用 vendor 持有的 DMA buffer。`base` 与 `phys_base` 相同只因为当前恒等映射；`mapped_len` 是页映射长度，用户态不得访问 `byte_len..mapped_len` 之外的未定义设备数据，内核也应在映射前清零尾页。

## 操作语义

`framebuffer()` 返回可变 slice，借用期受 `&mut self` 和外层设备锁约束。写 slice 只修改 backing buffer；`flush()`/`flush_region()` 才执行 transfer/submit，把内容送给宿主显示。

区域刷新参数属于像素坐标。当前实现将区域直接交给 vendor 层，没有在本 crate 明确验证 `x + width <= screen_width`、`y + height <= screen_height` 及加法溢出；调用者不能依赖 vendor 一定安全拒绝。扩展时应先用 checked_add 校验，零面积的语义也要固定。

## DMA HAL

HAL 逐页调用全局 frame allocator，验证 PPN 按递减顺序连续，取最低 PPN 后清零。它假定物理内存恒等映射且普通内核 buffer 的 VA 可直接作为 DMA PA。

GPU 版本使用 `saturating_mul` 计算 PPN 地址；若结果为零直接失败，但当前路径没有归还已经取得的 `ppns`，形成 frame 泄漏。`NonNull::new` 失败同样未回收。清零长度 `pages * PAGE_SIZE` 未 checked。应改成与 block HAL 相同的 checked 计算并由 RAII guard 统一失败回滚。

逐页取 frame 也不构成原子的连续页分配，SMP 插入和碎片都会导致偶发失败。改变恒等映射/IOMMU 后必须一起实现 alloc/dealloc、share/unshare、MMIO PA→VA 和 cache coherency。

## 锁与 framebuffer mmap 生命周期

设备注册层用 mutex 保证一次只有一个 framebuffer/flush 操作。不要在持有 framebuffer slice 时释放锁、调度或再次进入 display API；这会制造悬垂借用或死锁。

用户 framebuffer mmap 的生命周期可能长于一次系统调用。映射对象必须持有设备 `Arc`/lease，禁止仅复制裸 `phys_base` 后释放设备。销毁顺序是撤销用户映射和新借用、等待在途 flush、删除 registry 项，最后 Drop vendor resource 与 DMA。

## 扩展示例

新增安全的区域刷新时，先实现：

```text
right  = x.checked_add(width)  否则 InvalidParam
bottom = y.checked_add(height) 否则 InvalidParam
right <= info.width && bottom <= info.height
```

再调用 vendor。若允许多 framebuffer/resource，应让 `FramebufferInfo` 绑定具体 generation，mode set 后使旧 mmap lease 失效，而不是静默复用已释放 PA。

## 回归清单

- 零/错 MMIO、错误设备类型、feature/setup 失败；
- 1 页和多页 DMA、非连续/OOM、零地址/溢出失败后的 frame 基线；
- 常见和极端分辨率、stride/byte/mapped 算术、短 vendor buffer；
- 全屏 flush、四角最小区域、越界/溢出/零面积区域；
- BGRA 已知色块与宿主截图比对；
- framebuffer mmap 在进程退出、设备重建和并发 flush 下无 UAF；
- 反复构造销毁恢复 frame/heap 基线。

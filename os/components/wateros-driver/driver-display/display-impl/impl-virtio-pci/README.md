# VirtIO-PCI GPU 实现手册

[Display API](../../display-api/api-v0/README.md) · [LoongArch 探测](../../../driver-impl/impl-qemu-loongarch64-virt/README.md) · [MMIO GPU](../impl-virtio-mmio/README.md)

该实现为 bus 0 上的第一个 VirtIO GPU 分配 PCI BAR、开启 DMA，创建 2D framebuffer 并暴露 `DisplayDevice`。

## 核心对象

- `VirtioGpuPciProbeInfo` 保存成功 BDF 和 IDs；
- `VirtioGpuPciBarAllocator { next, end }` 单调分配 GPU 专用 MMIO 区间；
- `VirtioGpuPciDevice` 持有 `VirtIOGpu<VirtioGpuPciHal, PciTransport>` 及 `FramebufferInfo`。

GPU allocator 使用 `checked_next_power_of_two().max(16)`，对 size/对齐/上界均可失败；它仍没有 free 或 rollback。

## 调用链

```text
probe_first_from_ecam(config_base, allocator)
  -> MmioCam(Ecam) + PciRoot
  -> enumerate_bus(0)，筛选 VirtIO GPU
  -> assign_memory_bars
  -> command |= MEMORY_SPACE | BUS_MASTER
  -> PciTransport::new
  -> VirtIOGpu::new
  -> resolution + setup_framebuffer
  -> 构造 FramebufferInfo
  -> 返回 device + probe info
```

配置 base 和所有 BAR 必须已经在内核地址空间映射。当前仅扫描 bus 0，不穿越 bridge；I/O BAR 不配置，Below1MiB memory BAR 拒绝。成功前不得注册设备。

## 失败与事务边界

BAR allocator cursor、已写 BAR 和 PCI command 在后续 transport、feature 或 framebuffer 初始化失败时不会恢复。因此错误返回不代表硬件未改变，重试还会继续消耗地址窗口。应采用“保存旧配置并反序回滚”或“先计算完整配置计划，再提交”的事务设计；失败时尤其应清掉新增的 BUS_MASTER。

DMA HAL 与 MMIO GPU 版同样假设递减连续 PPN 和恒等映射。地址用 `saturating_mul`；得到零或空指针时目前不会归还已经取得的 PPN，是已知泄漏路径。清零长度也未 checked。修复时应抽出共享 VirtIO HAL，避免四类设备的细微错误继续分叉。

## framebuffer、锁与生命周期

像素格式固定 BGRA8888，stride/有效长度/页映射长度应使用 checked 算术并验证 vendor slice。全屏和区域 flush 都是同步 vendor 命令，外层 mutex 持锁期间不得等待 scheduler 或反向获取 VFS/用户映射锁。

当前区域刷新没有本地显式边界检查；必须在进入 vendor 前检查坐标加法和屏幕范围。用户 mmap 必须持有设备 lease，PCI BAR、DMA framebuffer 和 capability 映射要活到最后一个映射消失。

## 新增多显示支持

不能简单把 `probe_first` 改名。应收集稳定 BDF 列表，为每个设备事务分配独立 BAR，成功后按 BDF 顺序注册；一个 function 失败是跳过还是整批失败要形成平台策略。`/dev/fbN` 与 BDF 的对应关系应可诊断，mode generation 变化要使旧 mmap 安全失效。

## 回归清单

- 空 bus、错误 ID、modern/transitional capability、只在非零 bus 的设备；
- 32/64 位 BAR、Below1MiB/I/O/零/极大 BAR、GPU 窗口耗尽；
- 每个初始化失败点后验证 BAR、command、allocator cursor、frame/heap；
- DMA 非连续/OOM/地址溢出，以及失败泄漏修复；
- 分辨率和长度溢出、短 framebuffer、BGRA 色块；
- 全屏/合法边角/越界/零面积 flush；
- 用户 mmap + 进程退出 + 设备销毁的引用寿命；
- SMP 并发绘制由同一锁串行且无锁序反转。

# wateros-driver-display

[项目首页](../../../../README.md) · [内核工程](../../../README.md) · [系统架构](../../../../README.md#系统架构)

`wateros-driver-display` 是 WaterOS 的显示设备子系统。它只抽象“线性帧缓冲 + 主动刷新”，
不在驱动中实现窗口、字体或输入事件。绘制方写入 framebuffer 后必须调用 `flush()` 或
`flush_region()`，否则 QEMU 窗口不会更新。

## 模块分层

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 聚合门面 | `src/lib.rs` | 按 feature 再导出显示 API 与具体实现，提供 `supported_devices()` 与 `display_subsystem_claims_device()`。 |
| 显示设备 API | `display-api/api-v0/` | `PixelFormat`、`FramebufferInfo`、`FramebufferRegion`、`DisplayDevice` 与全局注册表。 |
| VirtIO MMIO 实现 | `display-impl/impl-virtio-mmio/` | RISC-V QEMU `virtio-gpu-device`。 |
| VirtIO PCI 实现 | `display-impl/impl-virtio-pci/` | LoongArch QEMU `virtio-gpu-pci`。 |

## 实现说明

- 显示 API 只描述线性帧缓冲与刷新操作，不感知 VirtIO、PCI 或具体绘制算法；上层 Canvas 在
  设备锁保护下修改缓冲，再显式调用 `DisplayDevice::flush` 提交给宿主显示设备。
- 像素格式固定为 `PixelFormat::Bgra8888`（每像素 4 字节：蓝、绿、红、透明度）。
- `FramebufferInfo` 同时区分：`byte_len`（可见字节数）、`mapped_len`（页对齐映射长度）、
  `phys_base`（DMA 物理起点，仅供设备 mmap 层）与 `base`（内核恒等映射地址，仅供诊断）。
- `DisplayDevice::flush_region` 默认安全退化为全屏刷新；VirtIO GPU 实现会覆盖该方法，只
  传输和提交指定矩形区域。
- `SharedDisplayDevice = Arc<Mutex<Box<dyn DisplayDevice>>>`，锁同时保护驱动状态和可写
  framebuffer。
- DMA framebuffer 由公共 `VirtioHal::dma_alloc` 从 linker 保留的 DMA pool 申请**物理连续、页对齐、已清零**的内存。
- 该模块由顶层 `gui`（内核绘制）或 `user-graphics`（通过 `/dev/fb0` 向用户态共享 DMA 页）
  feature 显式启用，两者互斥；默认比赛构建不会探测 GPU，也不会额外分配 framebuffer。
- `supported_devices()` 声明三个可绑定条目：`virtio,mmio`（`virtio-gpu-mmio`）、
  `pci1af4,1010`（transitional）、`pci1af4,1050`（modern）。

## 调用链路

引导期注册（RISC-V 为例）：

```text
probe_virtio_devices()
  -> display_subsystem_claims_device(compatibles, DeviceType::Display)
  -> VirtioGpuMmioDevice::from_mmio(mmio)
  -> register_display_device(SharedDisplayDevice)  // 返回稳定下标
```

上层刷新链路（用户图形为例）：

```text
Nano-X ioctl(FBIOPAN_DISPLAY)
  -> sys_ioctl() -> framebuffer_ioctl()
  -> FramebufferHandle::flush_device()
  -> DisplayDevice::flush() / flush_region()
  -> VirtIOGpu flush -> QEMU 窗口更新
```

## 各实现功能

### display-api / 显示设备 API

主要实现在 `display-api/api-v0/src/lib.rs`：

- 提供线性帧缓冲与主动刷新：`DisplayDevice` 通过 `info()` 报告分辨率/步长/像素格式，
  `framebuffer()` 借用可写帧缓冲，`flush()` 全屏提交，`flush_region()` 区域提交（默认安全退化
  为全屏）。
- 描述帧缓冲元数据：`FramebufferInfo` 同时给出可见字节数、页对齐映射长度、物理起点与内核
  诊断地址；`FramebufferRegion` 描述待提交矩形。
- 提供稳定注册表：`register_display_device` / `first_display_device` / `display_device_at` /
  `display_device_count`。

### impl-virtio-mmio / RISC-V VirtIO GPU

- 从 DTB 枚举得到的 MMIO 窗口初始化 VirtIO GPU（`VirtioGpuMmioDevice::from_mmio`）：建传输 →
  协商 → 查分辨率 → 分配 DMA framebuffer → 构造 `FramebufferInfo`。
- 通过公共固定 DMA pool 申请连续物理页；pool OOM 时整体回滚。
- 刷新直接转发 `VirtIOGpu::flush` / `flush_region`。

### impl-virtio-pci / LoongArch VirtIO GPU

- 走 PCI ECAM 枚举并初始化 VirtIO GPU（`probe_first_from_ecam`），为 BAR 分配 MMIO 地址并
  开启 `MEMORY_SPACE` / `BUS_MASTER`。
- 上层接口（`DisplayDevice`）与 RISC-V 完全相同，仅 transport 不同。

## 失败边界与回归

分辨率、stride、byte/mapped length任何溢出或短framebuffer都必须使构造失败且不注册；区域flush要拒绝坐标加法越界。回归覆盖DMA OOM/回滚、BGRA色块、四角/越界区域、用户mmap lease、SMP绘制锁，以及RV MMIO与LA PCI两条transport。

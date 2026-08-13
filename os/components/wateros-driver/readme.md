# wateros-driver

[项目首页](../../../README.md) · [内核工程](../../README.md) · [系统架构](../../../README.md#系统架构)

`wateros-driver` 是 WaterOS 的设备驱动聚合模块。它统一导出 block、character、display、input、
network 五个设备子系统，通过 `machine()` 选择当前机器驱动实现（QEMU RISC-V、LoongArch 或
dummy），并在内核引导期完成 DTB/PCI 扫描、设备注册与 devfs 同步。上层（syscall、VFS、MM、
用户图形）只依赖 `MachineDriver` 契约和子系统公共 API，不直接引用具体 transport 实现。

## 模块分层


| 层           | 路径                 | 职责                                                                                                                 |
| -------------- | ---------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| 聚合门面     | `src/lib.rs`         | 按 feature 导出五个子系统，提供`machine()`、`supported_device_entries()`、`init_after_boot()` 与 `test()`。          |
| 公共数据模型 | `driver-api/api-v0/` | `DeviceType`、`MmioRegion`、`IrqLine`、`SupportedDeviceEntry`、`DeviceInfo`、`DriverError` 与 `MachineDriver` 契约。 |
| 块设备       | `driver-block/`      | VirtIO 块设备（MMIO/PCI）、块缓存与 dummy 实现。                                                                     |
| 字符设备     | `driver-character/`  | UART(NS16550)、RTC、null 等字符设备与注册表。                                                                        |
| 显示设备     | `driver-display/`    | VirtIO GPU（MMIO/PCI），framebuffer 与主动刷新。                                                                     |
| 输入设备     | `driver-input/`      | VirtIO 键盘/平板（MMIO/PCI），原始输入事件。                                                                         |
| 网络设备     | `driver-network/`    | VirtIO 网卡（MMIO/PCI）与 dummy 实现。                                                                               |
| 机器驱动实现 | `driver-impl/`       | dummy、共享 DTB 解析、QEMU RV/LA 平台探测与注册。                                                                    |

五个子系统继续采用“聚合 crate → API crate → 实现 crate”的结构，并通过 feature 选择 transport：


| 子系统    | API                     | 实现                                                          | 详细文档                                     |
| ----------- | ------------------------- | --------------------------------------------------------------- | ---------------------------------------------- |
| block     | `block-api/api-v0/`     | `block-impl/impl-{block-cache,dummy,virtio-mmio,virtio-pci}/` | [`driver-block`](driver-block/README.md) |
| character | `character-api/api-v0/` | `character-impl/impl-{dummy,null-stub,rtc-stub,uart-16550}/`  | [`driver-character`](driver-character/README.md) |
| display   | `display-api/api-v0/`   | `display-impl/impl-{virtio-mmio,virtio-pci}/`                 | [`driver-display`](driver-display/README.md) |
| input     | `input-api/api-v0/`     | `input-impl/impl-{virtio-mmio,virtio-pci}/`                   | [`driver-input`](driver-input/README.md)     |
| network   | `network-api/api-v0/`   | `network-impl/impl-{dummy,virtio-mmio,virtio-pci}/`           | [`driver-network`](driver-network/README.md) |

## 实现说明

- 每个子系统通过 `supported_devices()` 静态声明可尝试绑定的设备（`SupportedDeviceEntry`），
  只用于 DTB/PCI 扫描阶段的 `compatible` 匹配，不决定初始化成败；多个子系统可同时声明
  匹配同一节点。
- 设备驱动只描述硬件能力并暴露领域 trait（如 `DisplayDevice`、`InputDevice`、
  `CharacterDevice`），不解析 syscall 号，也不把领域错误转换为 Linux errno；ABI 与用户指针
  留在 syscall 层。
- `machine()` 三选一：QEMU/OpenSBI RISC-V、QEMU LoongArch64 virt、或 dummy 占位；行为契约
  统一由 `MachineDriver` 表达，上层不再引用具体 impl crate。
- 引导期 `init_after_boot()` 依次：打印设备目录 → 扫描 DTB → 注册字符设备 → 按 VirtIO
  device id 分发注册 virtio 设备 → devfs 同步；失败只记日志，不向上返回错误（当前契约）。
- VirtIO device id 映射：`1` Network、`2` Block、`16` Display、`18` Input；DTB 中也可按
  `compatible` 精确匹配（如 `virtio,mmio`、`ns16550a`、`pci1af4,10xx`）。
- DMA 内存必须物理连续，由各实现中的 `Hal::dma_alloc` 向 frame allocator 申请连续帧；
  不连续时整体回滚并返回错误。
- display 与 input 由 `gui`（内核 GUI）或 `user-graphics`（用户态 fbdev/evdev）feature 显式
  启用，两者互斥；默认比赛构建不启用，不会探测 GPU/输入设备。
- 网络与块设备依赖 QEMU 挂载对应 virtio 设备；缺失时 `init_after_boot` 会输出警告日志。
- `driver-api` 的 `DriverError` 是跨子系统的错误分类（`InvalidDtb`/`Unsupported`/`IoError`
  等），不区分 errno 细节；驱动自检不访问真实硬件。

## 调用链路

引导期 bring-up（RISC-V 为例）：

```text
wateros_kernel_main()
  -> driver::machine().init_after_boot()   // wateros-driver::init_after_boot()
  -> 打印各子系统 supported_devices 目录
  -> enumerate::scan_device_info()          // 遍历 DTB 设备表，构造 DeviceInfo
  -> register::probe_character_devices()    // UART / RTC / null 注册
  -> register::probe_virtio_devices()       // 按 device id 分发到 block/network/display/input
  -> devfs::sync()                          // 同步 devfs 设备视图
```

设备绑定与注册：

```text
DTB 节点 / PCI 设备
  -> 与子系统 supported_devices() 的 compatible 匹配
  -> 构造具体设备（如 VirtioGpuMmioDevice::from_mmio）
  -> 子系统注册表 register_xxx_device()
  -> 上层通过子系统公共 API（如 display::first_display_device()）访问
```

上层使用（以显示为例）：

```text
VFS / 用户图形
  -> display::first_display_device()
  -> DisplayDevice::info() / framebuffer() / flush() / flush_region()
```

## 子系统实现功能

### driver-api / 公共数据模型

`driver-api/api-v0/src/lib.rs` 提供跨子系统共享的类型：

- `DeviceType`：Block / Character / Network / Display / Input / Unknown。
- `DeviceInfo`：DTB 节点名、`compatible` 列表、探测类型、MMIO 区间与中断线。
- `SupportedDeviceEntry`：子系统声明的“可绑定”设备描述（subsystem/name/compatible）。
- `DriverError` 与 `DriverResult<T>`：统一错误分类与返回值别名。
- `MachineDriver`：机器级契约（`init_after_boot`、`realtime_ns`、`test`）；每个 `driver-impl`
  profile 实现它，并通过 `machine()` 暴露单例。

### driver-block / 块设备

- `BlockCacheManager` 与 `CachingBlockDevice` 提供块缓存，`BlockCacheConfig` 可调参。
- `impl-virtio-mmio` / `impl-virtio-pci` 分别对接 RISC-V MMIO 与 LoongArch PCI 的 virtio-blk；
  `impl-dummy` 提供占位。
- DTB 声明支持 `virtio,mmio` 与 PCI transitional/modern 的 virtio-blk。

### driver-character / 字符设备

- `CharacterDevice` 注册表与 `first_character_device` / `with_character_device` 等访问接口。
- `impl-uart-16550`：NS16550 串口，`Ns16550Port` 与 `RegisterLayout`。
- `impl-rtc-stub`：实时钟；`impl-null-stub`：null 设备；`impl-dummy`：占位。
- DTB 声明支持 `ns16550a` / `ns8250`；`is_uart_compatible` 还识别 `snps,dw-apb-uart`。

### driver-display / 显示设备

见 [`driver-display/README.md`](driver-display/README.md)。要点：

- `FramebufferInfo` 区分可见字节、页对齐长度、物理起点与内核诊断地址。
- 像素格式固定 BGRA8888；绘制后必须 `flush()` / `flush_region()`，否则 QEMU 窗口不更新。
- 由 `gui` 或 `user-graphics` 显式启用，两者互斥。

### driver-input / 输入设备

见 [`driver-input/README.md`](driver-input/README.md)。要点：

- 保留 VirtIO/evdev 的原始 `type/code/value` 语义；驱动不负责键盘布局、鼠标加速或窗口命中。
- `pop_event()` 均为非阻塞；GUI/evdev 轮询任务无事件时 sleep，不忙等。

### driver-network / 网络设备

- `impl-virtio-mmio` / `impl-virtio-pci` 对接 RISC-V MMIO 与 LoongArch PCI 的 virtio-net；
  `impl-dummy` 提供占位。
- DTB 声明支持 `virtio,mmio` 与 PCI transitional/modern 的 virtio-net。

### driver-impl / 机器驱动实现

- `impl-dummy`：无硬件占位 profile。
- `impl-common`：共享 DTB 解析（如 `parse_irq`）。
- `impl-qemu-riscv64-virt`：RISC-V64/OpenSBI。模块 `enumerate`（扫描 DTB）、`register`
  （实例化并注册设备）、`devfs`（同步设备视图）、`uart`（平台 UART 接线）、`machine`/`test`。
- `impl-qemu-loongarch64-virt`：LoongArch64 virt。结构与 RISC-V 对应；virtio 设备走 PCI
  ECAM，需要为 BAR 分配 MMIO 地址并开启 `MEMORY_SPACE` / `BUS_MASTER`。

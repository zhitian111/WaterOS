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
- `driver-api` 的 `DriverError` 是跨子系统的错误分类，不直接等同于 Linux errno；transport
  必须区分暂未就绪、内存不足、范围越界、协议状态错误和设备 I/O 失败，避免上层误判重试或
  文件系统损坏策略。块设备硬件自检复用已经注册的设备实例，不会为同一 VirtIO 队列重复初始化。

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

`driver-api/api-v0/src/lib.rs` 提供跨子系统共享的数据模型：

- 统一设备分类：`DeviceType` 区分 Block / Character / Network / Display / Input / Unknown，供
  DTB/PCI 探测后按类型路由到对应子系统。
- 汇总一次扫描结果：`DeviceInfo` 携带节点名、`compatible` 列表、探测类型、MMIO 区间与中断线，
  作为绑定决策与诊断的输入。
- 声明可绑定设备：`SupportedDeviceEntry` 让每个子系统静态列出自己可尝试处理的 `compatible`，
  供扫描阶段精确匹配。
- 统一错误分类：`DriverError` / `DriverResult<T>` 不区分 Linux errno，但保留 `NotReady`、
  `NoMemory`、`OutOfRange`、`Protocol` 与 `IoError` 等对恢复策略有影响的语义。
- 定义机器级契约：`MachineDriver`（`init_after_boot` / `realtime_ns` / `test`）由每个
  `driver-impl` profile 实现，上层经 `machine()` 拿到单例，不感知具体平台。

### driver-block / 块设备

- 提供按块寻址读写：`BlockDevice` 以 LBA 按块读写，并默认实现任意字节对齐的 `read_bytes` /
  `read_prefix`；不支持写时返回 `Unsupported`。
- 提供块缓存加速：`BlockCacheManager` 以写穿 LRU 包装任意 `BlockDevice`，连续未命中合并为
  单次底层读，读数据二次命中准入，避免顺序扫描污染缓存；`capacity_blocks` 为 0 时透传。
- 支持两种 VirtIO transport：RISC-V MMIO 与 LoongArch PCI 各自初始化 virtio-blk 并注册；
  探测阶段按 `compatible` 匹配 `virtio,mmio` 与 PCI transitional/modern 声明。

### driver-character / 字符设备

- 提供字符流 I/O：`CharacterDevice` 支持 `read` / `write` / `poll_revents`，并可选支持
  `prepare_read` / `finish_read` 事务式读取（预约 → 提交/回滚）。
- 提供串口最小契约：`SerialPort` 封装单字节/批量写、阻塞读与非阻塞读，由
  `SerialPortCharacterDevice` 包装成 `CharacterDevice`。
- 提供 NS16550 串口：`impl-uart-16550` 统一处理 `Byte16550` 与 `DwApb32`（reg-shift=2）两种
  寄存器布局，平台只传基址与布局。
- 提供 RTC / null 内置设备：`impl-rtc-stub` 读取实时钟、`impl-null-stub` 丢弃写入，
  `register_builtin_character_devices` 统一注册；DTB 声明支持 `ns16550a` / `ns8250`。

### driver-display / 显示设备

见 [`driver-display/README.md`](driver-display/README.md)。要点：

- 提供线性帧缓冲与主动刷新：`DisplayDevice` 给出 `FramebufferInfo`、借用可写帧缓冲，并提供
  `flush()`（全屏）与 `flush_region()`（区域，默认退化全屏）提交画面。
- 区分字节语义：`FramebufferInfo` 同时给出可见字节数、页对齐映射长度、物理起点与内核诊断
  地址，供设备 mmap 与绘制层分别使用。
- 像素格式固定 BGRA8888；绘制后必须显式刷新，否则 QEMU 窗口不更新；由 `gui` 或
  `user-graphics` 显式启用，两者互斥。

### driver-input / 输入设备

见 [`driver-input/README.md`](driver-input/README.md)。要点：

- 保留原始输入语义：`InputDevice` 非阻塞 `pop_event()` 返回 evdev 兼容三元组
  `RawInputEvent`；驱动不负责键盘布局、鼠标加速或窗口命中。
- 自动识别设备类型：初始化时查询设备名、`EV_REL`/`EV_ABS` 能力位图与绝对轴范围，判断
  `Keyboard` / `Pointer` / `Unknown`，供 devfs 建立 `keyboard0` / `pointer0` 别名。
- 非阻塞 + 轮询友好：GUI/evdev 轮询任务无事件时 sleep，不忙等。

### driver-network / 网络设备

- 提供以太网帧收发：`NetworkDevice` 暴露 `send` / `receive` 完整 L2 帧，以及 `mac_address` /
  `mtu` / `is_link_up` 元数据；协议栈经注册表统一调度。
- 支持两种 VirtIO transport：RISC-V MMIO 与 LoongArch PCI 各自初始化 virtio-net 并注册；
  探测阶段按 `compatible` 匹配 `virtio,mmio` 与 PCI transitional/modern 声明。

### driver-impl / 机器驱动实现

- 具体设备能力由已选择的 QEMU 平台实现提供；未选择平台实现时，机器驱动入口明确报告未配置。
- 提供共享 DTB 解析：`impl-common` 封装 `parse_irq` 等跨平台解析辅助。
- 提供 QEMU RISC-V 平台接入：`impl-qemu-riscv64-virt` 经 `enumerate` 扫描 DTB、`register`
  实例化并注册设备、`devfs` 同步设备视图、`uart` 接线 UART。
- 提供 QEMU LoongArch 平台接入：结构与 RISC-V 对应；virtio 设备走 PCI ECAM，需要为 BAR
  分配 MMIO 地址并开启 `MEMORY_SPACE` / `BUS_MASTER`。

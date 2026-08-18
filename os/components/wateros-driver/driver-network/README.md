# wateros-driver-network

[项目首页](../../../../README.md) · [内核工程](../../../README.md) · [系统架构](../../../../README.md#系统架构)

`wateros-driver-network` 是 WaterOS 的网络设备子系统。它抽象“以太网帧收发 + MAC 地址”，
具体网卡实现注册到全局表后由协议栈统一调度。驱动只负责 L2 帧的收发，不实现 TCP/IP、ARP
或路由。

## 模块分层

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 聚合门面 | `src/lib.rs` | 按 feature 再导出网络 API 与具体实现，提供 `supported_devices()`、`network_subsystem_claims_device()` 与 `test()`。 |
| 网络设备 API | `network-api/api-v0/` | `NetworkDevice` 与全局注册表，`DEFAULT_MTU` 为 1500。 |
| VirtIO MMIO 实现 | `network-impl/impl-virtio-mmio/` | RISC-V QEMU `virtio-net-device`。 |
| VirtIO PCI 实现 | `network-impl/impl-virtio-pci/` | LoongArch QEMU `virtio-net-pci`。 |

## 实现说明

- `NetworkDevice` 只暴露以太网帧收发与链路元数据，不解析协议，也不转换 Linux errno；错误
  统一走 `DriverResult`。
- `send(buf)` 操作完整的 L2 帧（含目的/源 MAC 与 EtherType），由调用方构造；
  `receive(buf)` 返回实际字节数，`buf` 不足以容纳完整帧时返回 `DriverError::InvalidParam`。
- 接收缓冲区 `RX_BUF_LEN = 2048`（含 virtio net header），须不小于 `MIN_BUFFER_LEN`（1526）。
- DMA / HAL：与块设备共用 linker 保留的固定 DMA pool，DMA 内存物理连续、页对齐、已清零；
  普通 buffer 通过 HAL staging share/unshare。
- 各 transport（MMIO/PCI）对应 RISC-V / LoongArch：DTB 声明支持 `virtio,mmio` 与 PCI
  transitional/modern（`pci1af4,1000` / `pci1af4,1041`）。
- 缺失 virtio-net 时 `init_after_boot` 会输出警告日志，网络可能不可用。

## 调用链路

引导期注册（RISC-V 为例）：

```text
probe_virtio_devices()
  -> network_subsystem_claims_device(compatibles, DeviceType::Network)
  -> VirtioNetDevice::from_mmio(mmio)
  -> register_network_device(SharedNetworkDevice)  // 返回稳定下标
```

上层访问：

```text
协议栈
  -> first_network_device() / network_device_at(index)
  -> NetworkDevice::send / receive / mac_address / mtu / is_link_up
```

## 各实现功能

### network-api / 网络设备 API

主要实现在 `network-api/api/v0/src/lib.rs`：

- 提供以太网帧收发：`NetworkDevice` 实现 `send` / `receive`（完整 L2 帧），并提供
  `mac_address` / `mtu`（默认 1500）/ `is_link_up` 元数据；`buf` 不足以容纳完整帧时返回
  `InvalidParam`。
- 提供稳定注册表：`register_network_device` / `first_network_device` / `network_device_at` /
  `network_device_count`。

### impl-virtio-mmio / RISC-V VirtIO 网络

- 从 DTB 枚举得到的 MMIO 窗口初始化 virtio-net（`VirtioNetDevice::from_mmio`）。
- 通过恒等映射帧分配申请 DMA 内存（`VirtioMmioHal`），与块设备共用同一策略。

### impl-virtio-pci / LoongArch VirtIO 网络

- 走 PCI ECAM 枚举并初始化 virtio-net（`VirtioPciNetDevice`），为 BAR 分配 MMIO 地址并开启
  `MEMORY_SPACE` / `BUS_MASTER`。

## 并发与回归

网卡mutex内只做有界send/receive，不能反向进入协议栈、socket或scheduler；RX buffer和设备Arc覆盖poll生命周期。回归验证MAC/MTU/link、空RX、小buffer不得误消费、最大/连续帧、recycle失败、DMA OOM，以及RV/LA上的ARP、ICMP、UDP、TCP长流量和锁序。

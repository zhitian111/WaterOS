# wateros-driver-input

[项目首页](../../../../README.md) · [内核工程](../../../README.md) · [系统架构](../../../../README.md#系统架构)

`wateros-driver-input` 是 WaterOS 的输入设备子系统。它保留 VirtIO/evdev 的原始
`type/code/value` 语义，硬件驱动不负责键盘布局、鼠标加速或窗口命中；上层（`wateros-gui`
的 input bridge 或 `user-graphics` 的 evdev worker）负责把原始事件转成窗口系统事件。

## 模块分层

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 聚合门面 | `src/lib.rs` | 按 feature 再导出输入 API 与具体实现，提供 `supported_devices()` 与 `input_subsystem_claims_device()`。 |
| 输入设备 API | `input-api/api-v0/` | `InputDeviceKind`、`AbsoluteAxis`、`InputDeviceInfo`、`RawInputEvent`、`InputDevice` 与全局注册表。 |
| VirtIO MMIO 实现 | `input-impl/impl-virtio-mmio/` | RISC-V QEMU `virtio-keyboard-device`、`virtio-tablet-device`。 |
| VirtIO PCI 实现 | `input-impl/impl-virtio-pci/` | LoongArch QEMU `virtio-keyboard-pci`、`virtio-tablet-pci`。 |

## 实现说明

- 输入 API 只描述原始事件三元组 `RawInputEvent { event_type, code, value }`，不解析键盘布局、
  鼠标加速或窗口命中，也不转换 Linux errno。
- `InputDevice::pop_event()` 必须**非阻塞**：无数据时返回 `Ok(None)`；GUI/evdev 轮询任务在
  无事件时会 sleep，不会忙等。
- 驱动初始化时通过 `query_info` 查询设备名称、`EV_REL`/`EV_ABS` 能力位图与绝对 X/Y 范围，
  据此判断 `InputDeviceKind`（`Keyboard` / `Pointer` / `Unknown`）。
- `/dev/input/eventN` 保留每个驱动的稳定注册序号；`keyboard0`、`pointer0` 按
  `InputDeviceKind` 建立别名，不依赖键盘与平板在 QEMU 参数中的先后顺序。
- `wateros-gui` 的 input bridge 把原始事件转换为内核 GUI 事件；`user-graphics` 则由 VFS
  输入 worker 广播为 Linux 24 字节 `input_event`。该模块由 `gui` 或 `user-graphics` feature
  显式启用，两者互斥。
- `supported_devices()` 声明三个可绑定条目：`virtio,mmio`（`virtio-input-mmio`）、
  `pci1af4,1012`（transitional）、`pci1af4,1052`（modern）。

## 调用链路

引导期注册（RISC-V 为例）：

```text
probe_virtio_devices()
  -> input_subsystem_claims_device(compatibles, DeviceType::Input)
  -> VirtioInputMmioDevice::from_mmio(mmio)
  -> register_input_device(SharedInputDevice)  // 返回稳定下标
```

事件采集（用户图形为例）：

```text
user_graphics_input_worker()
  -> poll_input_once()
  -> InputDevice::pop_event()
  -> 按打开者广播入 EvdevClient 队列（Linux input_event）
  -> read / poll / select
  -> Nano-X KBD_Read() / MOU_Read()
```

## 各实现功能

### input-api / 输入设备 API

主要实现在 `input-api/api-v0/src/lib.rs`：

- 提供原始输入事件：`InputDevice` 非阻塞 `pop_event()` 返回 evdev 兼容三元组 `RawInputEvent`。
- 描述设备元数据：`InputDeviceInfo` 携带名称、类别与绝对轴范围，初始化后固定；`AbsoluteAxis`
  给出闭区间范围供坐标缩放。
- 区分设备类别：`InputDeviceKind` 的 `Keyboard` / `Pointer` / `Unknown` 供 devfs 别名与事件
  解释策略选择。
- 提供稳定注册表：`register_input_device` / `input_devices` / `input_device_at` /
  `input_device_count`。

### impl-virtio-mmio / RISC-V VirtIO 输入

- 从 DTB 枚举得到的 MMIO 窗口初始化 VirtIO 键盘/平板（`VirtioInputMmioDevice::from_mmio`），
  查询设备能力并构造 `InputDeviceInfo`。
- 通过公共 `VirtioHal` 从 linker 保留的固定 DMA pool 申请 DMA 内存，与其它 VirtIO 驱动共用策略。
- `pop_event()` 直接转发 `VirtIOInput::pop_pending_event()` 并转换为 `RawInputEvent`。

### impl-virtio-pci / LoongArch VirtIO 输入

- 走 PCI ECAM 枚举并初始化 VirtIO 键盘/平板（`probe_all_from_ecam`），为 BAR 分配 MMIO 地址
  并开启 `MEMORY_SPACE` / `BUS_MASTER`。
- 上层接口（`InputDevice`）与 RISC-V 完全相同，仅 transport 不同。

# wateros-driver-input

[项目首页](../../../../README.md) · [内核工程](../../../README.md) · [系统架构](../../../../README.md#系统架构)

输入子系统保留 VirtIO/evdev 的原始 `type/code/value` 语义，硬件驱动不负责键盘布局、
鼠标加速或窗口命中。`wateros-gui` 的 input bridge 可把原始事件转换为内核 GUI 事件；
`user-graphics` 则由 VFS 输入 worker 广播为 Linux 24 字节 `input_event`。

- RISC-V：`virtio-keyboard-device`、`virtio-tablet-device`，MMIO transport。
- LoongArch：`virtio-keyboard-pci`、`virtio-tablet-pci`，PCI transport。
- 所有 `pop_event()` 均为非阻塞接口，GUI/evdev 轮询任务无事件时会 sleep，不会忙等。
- `/dev/input/eventN` 保留每个驱动的稳定序号；`keyboard0`、`pointer0` 按
  `InputDeviceKind` 建立别名，不依赖探测顺序。

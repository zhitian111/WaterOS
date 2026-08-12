# wateros-driver-input

[项目首页](../../../../README.md) · [内核工程](../../../README.md) · [系统架构](../../../../README.md#系统架构)

输入子系统保留 VirtIO/evdev 的原始 `type/code/value` 语义，硬件驱动不负责键盘布局、
鼠标加速或窗口命中。`wateros-gui` 的 input bridge 负责把原始事件转换为稳定
`InputEvent`。

- RISC-V：`virtio-keyboard-device`、`virtio-tablet-device`，MMIO transport。
- LoongArch：`virtio-keyboard-pci`、`virtio-tablet-pci`，PCI transport。
- 所有 `pop_event()` 均为非阻塞接口，GUI 轮询任务无事件时会 sleep，不会忙等。

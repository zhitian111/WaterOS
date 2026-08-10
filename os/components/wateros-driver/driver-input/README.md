# wateros-driver-input

输入子系统保留 VirtIO/evdev 的原始 `type/code/value` 语义，硬件驱动不负责键盘布局、
鼠标加速或窗口命中。`wateros-gui` 的 input bridge 负责把原始事件转换为稳定
`InputEvent`。

- RISC-V：`virtio-keyboard-device`、`virtio-tablet-device`，MMIO transport。
- LoongArch：`virtio-keyboard-pci`、`virtio-tablet-pci`，PCI transport。
- 所有 `pop_event()` 均为非阻塞接口，GUI 轮询任务无事件时会 sleep，不会忙等。
- `input-api::hid` 提供无硬件依赖的 USB HID boot keyboard/mouse report 解码器，输出
  同一组 evdev 原始事件，供未来 USB host/PS2 适配层复用；USB 控制器传输、IRQ、DMA
  和热插拔顺序仍需目标板验证。

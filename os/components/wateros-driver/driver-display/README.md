# wateros-driver-display

显示子系统只抽象“线性帧缓冲 + 主动刷新”，不在驱动中实现窗口、字体或输入事件。

- `display-api/api-v0`：`FramebufferInfo`、`DisplayDevice` 与全局注册表。
- `display-impl/impl-virtio-mmio`：RISC-V QEMU `virtio-gpu-device`。
- `display-impl/impl-virtio-pci`：LoongArch QEMU `virtio-gpu-pci`。

当前像素格式固定为 BGRA8888。`FramebufferInfo` 同时区分可见字节数、页对齐映射
长度、物理起点和仅供内核诊断的虚拟地址。绘制方写入 framebuffer 后必须调用 `flush()` 或
`flush_region()`，否则 QEMU 窗口不会更新；默认 `flush_region` 会安全退化为全屏
刷新。该模块由顶层 `gui` 或 `user-graphics` feature 显式启用；前者由内核绘制，后者
通过 `/dev/fb0` 向用户态共享 DMA 页，两者互斥。默认比赛构建不会探测 GPU，也不会
额外分配 framebuffer。

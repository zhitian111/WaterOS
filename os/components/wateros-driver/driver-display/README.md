# wateros-driver-display

显示子系统只抽象“线性帧缓冲 + 主动刷新”，不在驱动中实现窗口、字体或输入事件。

- `display-api/api-v0`：`FramebufferInfo`、`DisplayDevice` 与全局注册表。
- `display-impl/impl-virtio-mmio`：RISC-V QEMU `virtio-gpu-device`。
- `display-impl/impl-virtio-pci`：LoongArch QEMU `virtio-gpu-pci`。

当前像素格式固定为 BGRA8888。绘制方写入 framebuffer 后必须调用 `flush()` 或
`flush_region()`，否则 QEMU 窗口不会更新；默认 `flush_region` 会安全退化为全屏
刷新。该模块由顶层 `gui` feature 显式启用，`display-demo` 是兼容别名。默认比赛
构建不会探测 GPU，也不会额外分配 framebuffer。

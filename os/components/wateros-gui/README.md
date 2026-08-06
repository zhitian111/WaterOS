# wateros-gui

`wateros-gui` 是独立于 GPU transport 的 `no_std + alloc` 内核窗口系统。它已取代
`os/src/gui.rs` 的一次性欢迎页，并把公共模型、软件合成、硬件显示和硬件输入分离。

## 目录与职责

- `gui-api/api-v0`：几何、RGBA 颜色、文本样式、输入事件、窗口和控件模型。
- `gui-impl/impl-software`：Canvas、ASCII 字体、shadow framebuffer、脏矩形、窗口
  合成、焦点、输入适配与全局 runtime。
- `src/lib.rs`：聚合导出，内核和其它组件只依赖这一层。

GUI 通过 `wateros-driver-display` 使用显示设备，通过 `wateros-driver-input` 接收原始
evdev 事件。绘制发生在 shadow buffer，提交时才按 `GUI runtime → display device` 的
固定锁顺序短暂锁定 framebuffer；设备锁、GUI 锁都不能跨调度或等待。

## 已实现能力

- 裁剪、alpha 混合、矩形边框、Bresenham 直线、圆、实心圆、多边形和 BGRA blit。
- 完整 ASCII 0x20..0x7e、小写、数字、符号、换行、字符/单词换行和九宫格对齐。
- 双缓冲及最多 16 个脏矩形；溢出自动合并，不丢画面更新。
- 多窗口 z 序、活动窗口、标题栏、关闭请求、拖动、鼠标命中和键盘焦点。
- Panel、Label、Button、ProgressBar、TextInput 五类控件。
- 有界输入/语义事件队列、按钮点击、Tab 切换、文本编辑、光标移动和提交事件。
- VirtIO 键盘/平板轮询，evdev → GUI 转换、US 键盘布局、修饰键和绝对坐标缩放。
- 可替换主题、显式脏区、指定显示器初始化、可关闭重建、运行快照和默认演示桌面。

## 运行时数据流

```text
virtio-input(MMIO/PCI) → RawInputEvent → InputBridge → InputEvent
                                                      ↓
调用方 ← GuiEvent ← Desktop/Widget ← GuiRuntime → ShadowSurface/DirtyRegions
                                                    ↓（短暂锁）
                                     DisplayDevice framebuffer + flush_region
```

## 使用

GUI 默认不进入比赛构建。QEMU 演示：

```bash
make run ARCH=rv PROFILE=pre EXTRA_FEATURES=gui
make run ARCH=la PROFILE=pre EXTRA_FEATURES=gui
```

`display-demo` 仍是兼容别名。启用 GUI 时 QEMU 同时挂载 VirtIO GPU、键盘和平板；
UART shell 留在启动终端，GUI 位于单独窗口。无桌面环境可使用
`GRAPHICS_BACKEND=none` 完成驱动启动回归。

## 公共 API 示例

```rust
gui::initialize()?;
let mut window = gui::Window::new(gui::WindowId(42),
                                  "Example",
                                  gui::Rect::new(40, 40, 360, 220));
window.add_widget(gui::Widget::button(gui::WidgetId(1),
                                      gui::Rect::new(20, 30, 120, 36),
                                      "Run"));
gui::add_window(window)?;
gui::render()?;

// 常驻 GUI 任务中：
gui::poll_hardware_input()?;
gui::process_pending_input()?;
while let Some(event) = gui::poll_event()? {
    // 按 WindowId / WidgetId 处理 Clicked、Submitted 等语义事件。
}
gui::render_if_dirty()?;
```

根内核已经提供刷新任务，普通调用方只需管理窗口和语义事件。直接调用
`with_runtime` 时回调必须是短操作，禁止在回调内等待、调度或再次调用全局 GUI API。

## 扩展点与当前边界

- 新硬件输入只需实现 `InputDevice`，窗口系统不感知 MMIO/PCI/PS2。
- 新显示后端只需实现 `DisplayDevice`；覆盖 `flush_region` 可得到真正的局部提交。
- API 模型与 `impl-software` 分离，可新增硬件加速 renderer 或不同 compositor。
- 当前是内核 GUI，不包含 `/dev/fb0`、用户态窗口协议和 Unicode 字体；字体为完整可打印
  ASCII，键盘布局为 US。
- VirtIO GPU 0.12 的 `flush()` 仍是全屏传输；GUI 已按脏区复制，驱动局部刷新可后续补齐。

硬件输入后端只需把键盘/鼠标事件转换为 `InputEvent` 并调用 `push_input()`；窗口系统
无需感知 VirtIO-MMIO、PCI 或未来开发板的具体输入控制器。

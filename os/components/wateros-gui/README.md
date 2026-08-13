# wateros-gui

[项目首页](../../../README.md) · [内核工程](../../README.md) · [系统架构](../../../README.md#系统架构)

`wateros-gui` 是独立于 GPU transport 的 `no_std + alloc` 内核窗口系统。它已取代
`os/src/gui.rs` 的一次性欢迎页，并把公共模型、软件合成、硬件显示和硬件输入分离。

## 模块分层

| 层 | 路径 | 职责 |
| --- | --- | --- |
| 聚合门面 | `src/lib.rs` | 上层只依赖本 crate；`api` 提供稳定数据模型，`impl-software` 提供窗口、控件、事件与双缓冲实现。 |
| GUI API | `gui-api/api-v0/` | 几何、RGBA 颜色、文本样式、输入事件、窗口和控件模型。 |
| 软件实现 | `gui-impl/impl-software/` | Canvas、ASCII 字体、shadow framebuffer、脏矩形、窗口合成、焦点、输入适配与全局 runtime。 |

## 实现说明

- GUI 通过 `wateros-driver-display` 使用显示设备，通过 `wateros-driver-input` 接收原始 evdev
  事件；窗口系统不感知 MMIO/PCI/PS2。
- 绘制发生在 shadow buffer，提交时才按 `GUI runtime → display device` 的固定锁顺序短暂锁定
  framebuffer；设备锁、GUI 锁都不能跨调度或等待。
- 双缓冲及最多 16 个脏矩形；溢出自动合并，不丢画面更新。
- 已实现能力：裁剪、alpha 混合、矩形边框、Bresenham 直线、圆、实心圆、多边形和 BGRA blit；
  完整 ASCII 0x20..0x7e、换行、字符/单词换行和九宫格对齐；多窗口 z 序、活动窗口、标题栏、
  关闭请求、拖动、鼠标命中和键盘焦点；Panel/Label/Button/ProgressBar/TextInput 五类控件；
  有界输入/语义事件队列、按钮点击、Tab 切换、文本编辑、光标移动和提交事件；VirtIO 键盘/平板
  轮询、evdev→GUI 转换、US 键盘布局、修饰键和绝对坐标缩放；可替换主题、显式脏区、指定显示器
  初始化、可关闭重建、运行快照和默认演示桌面。
- GUI 默认不进入比赛构建；与 `user-graphics` 互斥（两者都是 framebuffer 最终所有者）。
- 当前是内核 GUI，不包含 `/dev/fb0`、用户态窗口协议和 Unicode 字体；字体为完整可打印 ASCII，
  键盘布局为 US；VirtIO GPU 0.12 的 `flush()` 仍是全屏传输，GUI 已按脏区复制。
- 扩展点：新硬件输入只需实现 `InputDevice`；新显示后端只需实现 `DisplayDevice`（覆盖
  `flush_region` 可得局部提交）；API 模型与 `impl-software` 分离，可新增硬件加速 renderer。

## 调用链路

运行时数据流：

```text
virtio-input(MMIO/PCI) → RawInputEvent → InputBridge → InputEvent
                                                      ↓
调用方 ← GuiEvent ← Desktop/Widget ← GuiRuntime → ShadowSurface/DirtyRegions
                                                    ↓（短暂锁）
                                     DisplayDevice framebuffer + flush_region
```

典型调用方：

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
```

常驻 GUI 任务：`poll_hardware_input` → `process_pending_input` → `poll_event` →
`render_if_dirty`。直接调用 `with_runtime` 时回调必须是短操作，禁止在回调内等待、调度或再次
调用全局 GUI API。

## 各实现功能

### gui-api / GUI 模型

主要实现在 `gui-api/api-v0/src/`。

- `geometry.rs`：`Rect` / `Point` / `Size`；矩形提供 `intersection` / `union` / `intersects` 等
  命中与合并原语，供脏矩形合并和控件命中使用。
- `color.rs`：RGBA 颜色与常用常量。
- `text.rs`：文本样式（对齐、换行、九宫格）。
- `event.rs`：`InputEvent`（硬件输入转换后的语义输入）与 `GuiEvent`（Clicked/Submitted 等语义
  事件，按 WindowId/WidgetId 分发）。
- `widget.rs`：`Window` / `WidgetId` / `WindowId` 与 Panel/Label/Button/ProgressBar/TextInput
  控件模型。

### impl-software / 软件实现

主要实现在 `gui-impl/impl-software/src/`。

- `surface.rs`：`ShadowSurface { size, stride, pixels: Vec<u8> }`——CPU 内 BGRA8888 双缓冲绘制
  目标，`stride = width * 4`；`DirtyRegions` 最多保留 16 个区域（`MAX_DIRTY_REGIONS`），新区域
  与已有区域 `touches_or_overlaps` 时合并为并集，超过容量退化为一个包围矩形，保证永不丢更新。
- `runtime.rs`：`GuiRuntime`——单显示器实例，持有 `display: SharedDisplayDevice`、`surface`、
  `desktop`、`theme`、`dirty`、`input`/`output` 两个有界 `VecDeque`（容量 256）、`input_bridge`
  与帧计数。锁顺序固定为 `GUI runtime → display device`；`new` 时校验 `info.format == Bgra8888`
  且 `stride >= width * 4`，初始 `dirty.mark_all()`。
- `scene.rs`：`Desktop`——多窗口 z 序、活动窗口、焦点、命中与窗口合成；窗口拖动、标题栏、关闭
  请求。
- `input.rs`：`InputBridge`——VirtIO 键盘/平板 `RawInputEvent` → `InputEvent`；US 键盘布局、
  修饰键、绝对坐标缩放；调用方 `push_input()` 入队。
- `canvas.rs`：Canvas 绘制原语（裁剪、alpha 混合、Bresenham 直线、圆、多边形、BGRA blit）。
- `font.rs`：完整可打印 ASCII 0x20..0x7e 字体，字符/单词换行与九宫格对齐。
- `widget.rs`：Panel/Label/Button/ProgressBar/TextInput 五类控件的行为。
- `theme.rs`：可替换主题；`set_theme` 会 `dirty.mark_all()`。
- `global.rs`：全局 runtime 单例与 `initialize` / `render` / `poll_hardware_input` /
  `render_if_dirty` 等入口。
- `demo.rs`：默认演示桌面。

## 使用

GUI 默认不进入比赛构建。QEMU 演示：

```bash
make run ARCH=rv PROFILE=pre EXTRA_FEATURES=gui
make run ARCH=la PROFILE=pre EXTRA_FEATURES=gui
```

`display-demo` 仍是兼容别名。启用 GUI 时 QEMU 同时挂载 VirtIO GPU、键盘和平板；UART shell
留在启动终端，GUI 位于单独窗口。无桌面环境可使用 `GRAPHICS_BACKEND=none` 完成驱动启动回归。

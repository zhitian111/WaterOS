# wateros-gui — 实现与扩展指南

## 分层约束

```text
gui-api/api-v0                 只放平台无关数据模型
gui-impl/impl-software         软件绘制、场景、合成和输入适配
driver-display / driver-input  硬件 transport 与设备注册
os/src/main.rs                 生命周期、刷新任务和演示业务事件
```

不要在 API crate 中引用 VirtIO、调度器或全局设备表。不要让显示/input 驱动了解
`Window` 或 `Widget`。

## 新增显示后端

1. 实现 `wateros-driver-display::DisplayDevice`。
2. 提供有效的 `FramebufferInfo` 和设备锁期间可写的线性 framebuffer。
3. 注册到 `register_display_device`。
4. 至少实现全屏 `flush`；支持局部传输时覆盖 `flush_region`。
5. 保证 framebuffer 生命周期覆盖设备注册期，且 DMA 映射满足平台要求。

GUI 当前接受 BGRA8888。增加像素格式时，应先扩展 `PixelFormat` 和集中编码函数，禁止在
各控件里散布通道顺序判断。

## 新增输入后端

1. 实现 `wateros-driver-input::InputDevice`。
2. `pop_event` 必须非阻塞，不得在设备锁内等待或调度。
3. 事件使用 Linux evdev 三元组 `event_type/code/value`。
4. 注册到 `register_input_device`；`InputBridge` 会自动发现注册表增长。
5. 非 evdev 硬件可在 GUI impl 新增独立适配器，最终只产生 `InputEvent`。

## 新增控件

1. 在 API 的 `WidgetKind` 增加拥有型状态结构，避免存放裸回调或 impl 私有类型。
2. 在 `scene.rs::render_widget` 增加绘制分支。
3. 在命中、焦点和键盘路径中补状态机；业务动作通过 `GuiEvent` 上报。
4. 状态变化必须登记控件屏幕矩形，无法精确判断时可 `mark_all` 保证正确性。
5. 为绘制和事件路由添加 host 单元测试。

## 新 renderer / compositor

新增 `gui-impl/impl-*`，只依赖 `gui-api/api-v0` 与稳定 driver API，并由聚合 crate feature
选择。不要让根内核直接依赖实现 crate。软件实现中的 `Desktop` 是私有策略，公共的
`Window`/`Widget` 才是实现间契约。

## 锁与调度

- 全局顺序：`GUI runtime → input device（单次 pop）/display device（单次提交）`。
- 不得持 GUI、输入、显示锁进入 sleep/yield、syscall、VFS 或用户回调。
- 单帧硬件输入最多读取 128 个原始事件，防止输入风暴饿死合成任务。
- 业务处理通过 `poll_event` 在锁外完成；需要更新时再调用短 API。

## 启动接线

设备注册完成后执行 `initialize → add_window/install desktop → render`，然后创建唯一刷新
任务。刷新循环负责硬件输入、事件处理、dirty render 和短睡眠。不得为每个 CPU 创建
GUI 刷新任务。


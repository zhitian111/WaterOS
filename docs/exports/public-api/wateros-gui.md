# wateros-gui — 公共 API

## 稳定数据模型

| 分类 | 主要类型 |
|------|----------|
| 几何 | `Point`、`Size`、`Rect`、`Insets` |
| 颜色 | `Color`，含 RGBA 混合和 BGRA8888 编解码 |
| 文本 | `TextStyle`、`TextMetrics`、`TextWrap`、`TextAlign`、`VerticalAlign` |
| 输入 | `InputEvent`、`PointerEvent`、`KeyEvent`、`KeyCode`、`KeyModifiers` |
| 输出 | `GuiEvent`、`GuiEventKind` |
| 窗口 | `Window`、`WindowId` |
| 控件 | `Widget`、`WidgetId`、`Panel`、`Label`、`Button`、`ProgressBar`、`TextInput` |

ID 是调用方提供的拥有型标识，GUI 不推断业务语义。窗口控件数据结构不依赖具体
renderer，因此可由其它实现消费。

## 全局 runtime

| API | 作用 |
|-----|------|
| `initialize()` / `initialize_on(index)` | 在首个或指定显示设备建立 runtime |
| `shutdown()` | 销毁 runtime，允许重新初始化 |
| `add_window` / `remove_window` | 修改窗口树 |
| `set_label_text` / `set_progress` | 定向更新控件并登记脏区 |
| `set_theme` / `mark_dirty` | 更换主题或显式登记重绘区域 |
| `push_input` | 注入平台无关输入事件 |
| `poll_hardware_input` | 非阻塞读取已注册输入设备 |
| `process_pending_input` | 命中测试、焦点/窗口状态变更与语义事件生成 |
| `poll_event` | 读取一个业务语义事件 |
| `render` / `render_if_dirty` | 合成并提交画面 |
| `runtime_snapshot` | 查询尺寸、窗口数、帧数、队列和丢弃计数 |
| `with_runtime` | 高级短操作入口；禁止等待、调度和递归调用全局 API |

## 软件绘制 API

`Canvas` 提供 `put_pixel`、`fill_rect`、`stroke_rect`、`draw_line`、`draw_circle`、
`fill_circle`、`draw_polyline`、`draw_polygon`、`fill_polygon`、`blit_bgra`、
`measure_text`、`draw_text`。`set_clip` 返回旧裁剪区，调用方必须用 `restore_clip`
恢复。

`GuiError` 区分未初始化、重复初始化、无显示器、无效 surface、窗口/控件不存在、队列
满和显示提交失败。


# Software GUI Runtime 离线开发手册

[GUI 总览](../../README.md) · [GUI API](../../gui-api/api-v0/README.md) · [Display API](../../../wateros-driver/driver-display/display-api/api-v0/README.md) · [Input API](../../../wateros-driver/driver-input/input-api/api-v0/README.md)

本 crate 是 WaterOS 的单显示器软件 GUI：窗口和控件先绘制到内核堆中的 BGRA8888
shadow surface，再把脏矩形复制到显示设备 framebuffer。它不依赖 VirtIO transport，
但依赖 display/input 注册表提供的抽象设备。

## 1. 文件与职责

| 文件 | 职责 |
|---|---|
| `global.rs` | `Mutex<Option<GuiRuntime>>` 全局实例和公开薄封装 |
| `runtime.rs` | display/surface/desktop/队列所有权、输入处理和 present |
| `surface.rs` | BGRA shadow buffer 与最多 16 个 dirty region |
| `canvas.rs` | 裁剪、alpha、矩形、线、圆、多边形和 blit |
| `font.rs` | 内嵌 5×7 ASCII 字体、换行和测量 |
| `scene.rs` | 窗口 z-order、命中、焦点、capture、拖拽和控件状态机 |
| `input.rs` | Linux evdev 原始事件到 GUI 输入事件的转换 |
| `theme.rs` | 软件 renderer 的颜色主题 |
| `demo.rs` | 只用公开 API 构建的默认桌面和动画示例 |

实现是 `#![no_std]`，但使用 `alloc`。shadow surface、窗口/字符串/队列、文本布局和多边形
交点都会分配；“software”不等于“无堆分配”。

## 2. 核心对象图与所有权

```text
static RUNTIME: spin::Mutex<Option<GuiRuntime>>
  └─ GuiRuntime
     ├─ display: Arc<Mutex<dyn DisplayDevice>>
     ├─ surface: ShadowSurface { Size, packed stride, Vec<u8> }
     ├─ desktop: Desktop { Vec<Window>, focus/capture/drag/caret }
     ├─ theme: Theme
     ├─ dirty: DirtyRegions { [Rect; 16], len }
     ├─ input: VecDeque<InputEvent>       capacity policy=256
     ├─ input_bridge: InputBridge { Vec<DeviceState> }
     └─ output: VecDeque<GuiEvent>        capacity policy=256
```

`GuiRuntime` 持有 display 的共享引用，不拥有设备注册表。`shutdown()` 从全局 slot 中取走
runtime，随后释放 shadow surface、窗口树、事件队列和这份 display `Arc`；它不会注销或
关闭显示/输入设备。

`initialize_on(index)` 在 GUI 全局锁内查找 display 并构造 runtime。重复初始化返回
`AlreadyInitialized`；无设备返回 `NoDisplay`。默认 `initialize()` 使用 index 0。

## 3. framebuffer 契约

初始化只接受：

- `width > 0 && height > 0`；
- `PixelFormat::Bgra8888`；
- device stride 至少为 `width * 4`；
- shadow 的 `width*4` 和 `stride*height` 可表示为 `usize`。

`ShadowSurface` 始终是紧密排列的 `stride=width*4`，创建时用零填充。真实 framebuffer
可以有行尾 padding；`copy_region` 分别使用 shadow stride 和 device stride，逐行只复制
`region.width*4` 字节，不覆盖 padding。

初始化没有永久映射 framebuffer slice；每次 present 都重新调用 `display.framebuffer()`。
若 framebuffer 实际长度不足，slice 边界检查返回 `InvalidSurface`，不会越界写。

GUI 与 user-graphics 不能无协调地同时拥有/写同一个 framebuffer。引入用户态 mmap 前
必须定义独占 owner、切换协议或 compositor 提交协议。

## 4. 全局锁与锁顺序

所有全局 API 最终进入：

```text
with_runtime
  -> lock RUNTIME
  -> &mut GuiRuntime 上执行闭包
  -> unlock RUNTIME
```

允许的内部顺序只有：

```text
GUI runtime lock -> display device lock
GUI runtime lock -> 单次 input device lock/pop -> 立即释放
```

display/input 回调不得反向调用 GUI，否则会自锁。外部传给 `with_runtime` 的闭包必须短，
不得睡眠、调度或自行获取 display 锁。`render()` 是实现内部认可的 GUI→display 路径，
不要在回调中先持另一个设备锁再调用它。

当前输入转换、完整场景重画、文本/多边形分配和 framebuffer 复制都发生在 GUI 全局锁
内。大分辨率或复杂场景会延长锁持有时间；优化时应先做快照/双缓冲所有权设计，不能简单
解锁后继续借用 `Desktop`。

## 5. dirty region 数据结构

`DirtyRegions` 维护屏幕边界及 `[Rect; 16]`：

1. `add` 先裁剪到 surface，完全在屏外则忽略；
2. 与新矩形接触或重叠的已有区域会被反复 union；
3. 未满 16 个时保存为独立区域；
4. 已满时把全部区域和新区域退化为一个总包围矩形；
5. `take()` 分配一个 `Vec<Rect>` 返回当前列表，并立刻清空内部状态。

容量溢出只增加重绘/复制面积，不丢更新。矩形“接触”也会合并。`mark_all()` 覆盖为一个
全屏区域；主题、窗口增删以及任何会移动光标/窗口的输入都走全屏 dirty。

当前 renderer 并不按 dirty region 局部绘制：只要 dirty 非空，就先在 shadow surface
完整重画桌面；dirty 仅限制 shadow→framebuffer 的复制范围。这保证透明、z-order 和窗口
移动正确，但 CPU 绘制成本仍与全屏场景复杂度相关。

## 6. render/present 调用链及失败恢复

```text
render_if_dirty
  -> poll_hardware_input()       每轮最多消费 128 个 raw event
  -> process_pending_input()     更新 Desktop、生成 GuiEvent、标 dirty
  -> render()
     -> dirty.take()
     -> Canvas(shadow) 上完整 Desktop::render
     -> frames_rendered += 1
     -> lock display
     -> display.framebuffer()
     -> 对每个 dirty rect: copy_region
     -> union 所有 rect -> display.flush_region(union)
     -> frames_presented += 1
```

无 dirty 时返回 `Ok(false)`。成功 present 返回 `Ok(true)`。framebuffer 获取、任一复制或
flush 失败时，把本次所有 region 重新 `add` 回 dirty 并返回错误，下一帧可以重试；
`frames_rendered` 已增加而 `frames_presented` 不增加。

失败不具备 framebuffer 事务性：前几个 region 可能已经复制，或 flush 前 framebuffer 已
全部更新。重新标脏保证最终一致，不能保证失败瞬间没有撕裂。flush 只接收所有 region 的
union，设备可能刷新比逐区复制更大的范围。

## 7. Canvas 像素和裁剪

`Canvas` 借用 shadow 的 `&mut [u8]`，保存 stride、完整 bounds 和当前 clip。所有图元最终
通过边界检查后的 slice 或 `put_pixel` 写入：

- 像素编码是 B,G,R,A；写回 alpha 固定为 255；
- alpha=255 直接覆盖，其他 alpha 用 `Color::blend_over` 与现有像素混合；
- opaque `fill_rect` 按行快速写，透明矩形逐像素混合；
- 直线使用 Bresenham，圆使用中点/整数平方根，均无需浮点；
- `fill_polygon` 用奇偶扫描线，支持凹多边形，自交图形按奇偶规则；
- `blit_bgra` 不做 alpha blend，只复制 BGRA；源矩形负坐标直接拒绝，短源 buffer 会停止
  后续行而不报错。

`set_clip(new)` 使用 `surface.bounds ∩ new`，不是 `old_clip ∩ new`；嵌套控件必须保存返回
的旧 clip，并在结束时 `restore_clip(old)`。忘记恢复会裁掉后续兄弟控件。

`fill_polygon` 每次调用分配 `Vec<i32>` 保存扫描线交点。若要用于高频动画，可让 Canvas
借用调用方 scratch buffer 或引入有界数组，并为顶点上限定义错误语义。

## 8. 文本布局

字体固定为 5×7 glyph，水平 advance=6、行 advance=8，再乘 `TextStyle.scale.max(1)`。
完整覆盖 ASCII `0x20..=0x7e`，其他 Unicode 字符绘制为问号，但布局仍按 Rust `char`
计一个 glyph。

`NoWrap` 截断到一行容量；`Character` 按字符切行；`Word` 按空白分词，超长单词退化为
字符换行。显式 `\n` 总会先拆行。水平/垂直对齐在裁剪后的 bounds 内计算。

`measure_text` 和 `draw_text` 都会通过 `layout_lines` 创建 `Vec<String>`；draw 并非无分配。
内核 heap 紧张时 GUI 可能触发全局分配失败。扩展 UTF-8 字体需要同时修改 glyph lookup、
字宽测量、cursor 像素位置和换行，不能只替换字模。

## 9. Desktop、z-order 与命中测试

`windows: Vec<Window>` 顺序就是从底到顶，末项为最顶窗口。新增窗口会把旧窗口设为
inactive、将新窗口置顶；点击任意窗口部位调用 `bring_to_front`。绘制从头到尾，命中从
尾到头，控件命中也从尾到头。

命中优先级是 close → title bar → enabled/visible widget → window body → desktop。
窗口可见性影响绘制和命中；控件必须同时 visible、enabled 才能命中/Tab 聚焦。

当前没有以下保护：

- 不拒绝重复 `WindowId` 或同窗口内重复 `WidgetId`，查找会命中第一个；
- 拖动窗口不限制在屏幕内；
- 删除窗口只清除其 focus，没有立即清除同窗口的 capture/drag；后续释放/移动会逐步失效，
  但在此之前状态仍是陈旧的；
- 删除顶层窗口后只激活新的末项，没有生成 focus 事件。

新增窗口管理 API 时应先定义 ID 唯一性、焦点迁移和删除期间 capture 取消语义。

## 10. 指针、focus、capture 与拖拽状态机

左键按下时先把目标窗口置顶：

- desktop/body/title bar 会清 focus；
- close 只产生 `CloseRequested`，runtime 不自动删除窗口；
- movable title bar 建立 `WindowDrag { window, pointer-window_origin }`；
- widget 建立 focus 和 pointer capture；button 同时置 `pressed=true`。

移动事件若存在 drag，就用保存 offset 更新窗口 origin。左键释放取消 drag 和 capture；
button 只有释放点仍命中同一 widget 才生成 `Clicked`，随后清 pressed。

focus 改变按顺序产生旧控件 `FocusLost`、新控件 `FocusGained`。Tab 的候选顺序是窗口
从顶到底、每个窗口控件正序，仅 Button/TextInput 可聚焦。Space 激活已聚焦 Button，
Enter 对任意已聚焦控件产生 `Submitted`。

Tick 每 30 个 frame 切换 caret，相位变化且存在 focus 时才标画面改变。调用方若不注入
`InputEvent::Tick`，输入框光标不会闪烁。

## 11. TextInput 的 UTF-8 不变量

`TextInput.cursor` 是字符串的 UTF-8 字节偏移，不是字符序号。插入前会把超界/非边界
cursor 向前修正；左右移动、Backspace/Delete 都按 `char_indices`/`len_utf8` 移动，正常
状态不会切断字符。`maximum_chars` 按 Unicode scalar 数量限制。

但 render 和部分删除路径假定现有 cursor 已在字符边界。若外部直接构造/修改公开 API
对象并给出非法 cursor，`input.text[..cursor]` 可能 panic。新增 setter 时应集中规范化：
`cursor=min(len)` 后向前寻找 `is_char_boundary`，并为非法输入增加测试。

password 显示按字符数生成同数量 `*`，不会泄漏 UTF-8 字节长度，但仍会暴露字符数量。

## 12. 输入桥接

`InputBridge` 按输入注册表长度发现追加设备，每个 `DeviceState` 保存设备 Arc、kind、绝对
轴范围、指针位置、pending move 和修饰键。当前假定注册表只追加，不处理设备删除、重排
或重新连接后的状态重置。

每次 `poll(size, budget)` 对每个设备循环非阻塞 `pop_event`，设备锁只覆盖一次 pop；总共
最多消费 budget 个 raw event。runtime 固定传 128。返回数量是生成的 GUI event 数，
可能多于 raw 数，因为一次按键可同时生成 Key 和 Text。

- ABS 轴按 `[minimum, maximum] -> [0, extent-1]` 缩放；无范围则直接 clamp；
- REL 坐标用饱和加法并裁到屏幕；
- pointer move 延迟到 `SYN_REPORT` 合并发出；
- wheel 立即生成 Scroll；
- key value 0=release、1=press、2=repeat；
- 支持 Shift/Ctrl/Alt/Super/CapsLock 与 US 键盘字符映射；Ctrl/Alt 下不生成 Text；
- 未知按键保留为 `KeyCode::Unknown(raw_code)`。

当前 scene 把滚轮 `vertical: i32` 直接 `as u32` 放进 `ValueChanged`，负滚动会变成很大的
无符号值。这是已知语义缺口；新增滚动控件前应扩展有符号事件类型或显式方向/幅度字段。

## 13. 队列容量与丢弃策略

input/output 的初始和逻辑上限均为 256：

- `push_input` 在满时拒绝新事件、`dropped_input += 1` 并返回 `QueueFull`；
- 硬件 poll 忽略这个错误，所以 event storm 时丢最新 GUI 输入，但统计会增长；
- 每处理完一个 input，output 若超过 256 就从队头丢最旧事件；
- output 丢弃没有单独统计，snapshot 只提供 pending 数量；
- `poll_event` 从 output 队头取一个事件。

`VecDeque::with_capacity(256)` 不是绝对内存上限；实现逻辑控制长度，但初始化仍需 heap。
若 API 一次输入产生多个 output，队列可在本次处理内短暂超过 256，随后立即裁掉旧项。

## 14. 新增控件实例

以 Checkbox 为例，不能只补 render 分支：

1. 在 `gui-api/api-v0` 增加状态结构、`WidgetKind::Checkbox` 和构造器；
2. 定义 enabled/visible/focusable、点击和 Space 的行为；
3. `scene::render_widget` 绘制框、选中标记与文本；
4. pointer release 在 capture 且 still-inside 时切值并产生明确 `ValueChanged`；
5. `focus_next` 和 `activate_focused_*` 纳入该类型；
6. 提供 `set_checked(window,id,value)`，只在值变化时返回 widget screen rect 标脏；
7. 从 `GuiRuntime`、`global.rs`、聚合 GUI crate 逐层再导出；
8. 测试重叠窗口命中、disabled、capture 外释放、键盘激活、删除窗口和队列满。

若要让用户态通过 syscall 控制 GUI，还要先解决权限和所有权：用户指针复制、窗口归属
PID、ID 命名空间、进程退出清理、字符串长度上限、事件阻塞/轮询及 framebuffer 隔离，
不能直接把全局 `add_window` 暴露给任意进程。

## 15. 故障定位

| 现象 | 优先检查 |
|---|---|
| `NoDisplay` | display 注册顺序、index 和 board feature |
| `InvalidSurface` | format、stride、尺寸乘法、framebuffer 实际 slice 长度 |
| `DisplayFailure` | framebuffer 获取和 `flush_region`；dirty 应仍为 true |
| 有输入无移动 | 设备是否发送 `SYN_REPORT`、ABS axis 范围和 poll budget |
| 点击落到下层 | windows/widgets Vec 顺序、visible/enabled、坐标从 content 到 screen 的偏移 |
| 中文显示 `?` | 当前字体只覆盖 printable ASCII，不是 UTF-8 解码失败 |
| GUI 导致 heap OOM | shadow 大小 `w*h*4`、文本临时 String/Vec、多边形交点、窗口/队列 |
| 更新未显示 | setter 是否返回并添加 dirty、render 错误是否被调用方吞掉 |
| 部分画面撕裂 | 多 region copy 后单次 union flush 不具事务性 |

## 16. 自回归矩阵

单元/假设备测试至少覆盖：

- shadow 尺寸溢出、BGRA 编码、alpha 混合和 stride padding；
- clip 外不写、嵌套 clip 恢复、负坐标、短 blit source；
- dirty 裁剪、接触合并、超过 16 区退化、失败后完整重入队；
- framebuffer 第 N 区复制失败、flush 失败、成功计数和空 dirty；
- z-order、重复 ID、visible/enabled、focus 顺序、capture 外释放和窗口拖动；
- UTF-8 插入/左右/Backspace/Delete、非法 cursor 防御、maximum_chars；
- ABS 两端映射、REL clamp、SYN 合并、repeat、Caps/Shift、未知键；
- input 满时丢最新及统计，output 满时丢最旧；
- 设备注册表追加发现和模拟设备错误；
- initialize/shutdown/reinitialize 及多线程 API 串行化。

从 `os/` 执行：

```sh
cargo test --manifest-path components/wateros-gui/gui-impl/impl-software/Cargo.toml
python3 scripts/maintenance/check_offline_docs.py
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```

真机/QEMU 还需人工验证鼠标两端坐标、键盘文本、窗口拖拽、连续动画、失败重试及 GUI 与
TTY/console 同时运行时的锁延迟。

# GUI API v0 开发手册

[GUI 总览](../../README.md) · [软件实现](../../gui-impl/impl-software/README.md)

这是无硬件依赖、版本化的拥有型 GUI 数据模型。它只依赖 `core`/`alloc`，描述几何、颜色、
文本、输入事件、语义事件、窗口和控件；不引用 framebuffer、VirtIO、调度器、VFS 或
syscall。当前软件 renderer 直接消费这些公开字段，因此改变字段语义等同于 API 变更。

## 1. 文件和依赖方向

| 文件 | 公共模型 |
|---|---|
| `geometry.rs` | `Point`、`Size`、`Insets`、`Rect` |
| `color.rs` | 硬件无关 RGBA `Color` 和 BGRA 编解码 |
| `text.rs` | 对齐、换行、样式和测量结果 |
| `event.rs` | 指针/键盘输入及控件语义事件 |
| `widget.rs` | ID、五种控件、`Widget`、`Window` |
| `lib.rs` | 再导出、`GuiError`、`GuiResult` |

```text
应用/内核 GUI 调用方
  -> gui-api-v0
     <- software renderer 实现状态机和绘制
        -> display/input device API
```

API 类型可由其它 renderer 使用，但公开字段意味着调用者也能构造不满足软件实现内部
不变量的对象；跨边界入口必须校验，不能假定构造器是唯一来源。

## 2. 几何契约

`Point` 使用 `i32`，允许窗口/图元在屏幕左上之外；`Size` 使用 `u32`。`Size::is_empty`
只要任一维为 0 就为真。矩形统一是半开区间：

```text
[origin.x, right) × [origin.y, bottom)
```

因此 `Rect(0,0,10,10)` 包含 `(9,9)`，不包含 `(10,9)`；两矩形只共享边界时
`intersection=None`。这对像素循环、命中测试和 dirty merge 都很重要。

方法语义：

- `right/bottom` 用 i64 中间值并 clamp 到 i32；
- `intersection` 返回非空交集，否则 `None`；
- `union` 把空矩形视为单位元；
- `translated` 对 origin 使用 `i32::saturating_add`；
- `inset` 用饱和减法缩小 size，padding 大于尺寸时得到空矩形。

当前实现只适用于现实屏幕大小这一受控域。极端 `origin≈i32::MIN`、超大 `u32 size` 时，
`union/intersection` 的 `(right-left) as u32` 前仍可能发生 i32 减法溢出；`inset` 把
`u32` padding `as i32` 也可能绕成负数。处理不可信用户参数前应先限制坐标/尺寸到
`i32::MAX` 可表达范围，或把所有中间运算改为 i64 并显式返回错误。

## 3. 颜色

`Color` 的逻辑字段顺序是 RGBA；`to_bgra8888` 输出内存字节 `[blue,green,red,alpha]`，
`from_bgra8888` 相反。`rgb` 固定 alpha=255，另有 `TRANSPARENT/BLACK/WHITE`。

`foreground.blend_over(background)` 对 RGB 做带 `+127` 的整数四舍五入，忽略 background
自身 alpha，结果总是 alpha=255。它的契约是“覆盖到不透明背景”，不是完整 Porter-Duff
通用合成。若要支持多层半透明 surface，必须重新定义预乘/非预乘 alpha 及输出 alpha。

## 4. 文本模型

`TextStyle` 包含 foreground、可选 background、`scale: u8`、水平/垂直对齐和
`NoWrap/Character/Word`。API 不规定具体字体、glyph 宽度或 Unicode 覆盖；这些属于
renderer。当前软件实现把 scale=0 当 1，并只画 printable ASCII。

`TextMetrics { width,height,lines }` 是 renderer 在给定 bounds 内的结果，不保证自然尺寸
未被裁剪。调用者不能用一个 renderer 的 metrics 推断另一个 renderer 的精确像素布局。

## 5. 输入事件与语义事件

输入层：

- `PointerEvent` 总携带当前屏幕坐标，kind 为 Move、Button 或带符号 Scroll；
- `PointerButton::Other(u16)` 保留未知 evdev 按钮编码；
- `KeyCode` 是布局处理后的逻辑键，未知键保留 raw code；
- `KeyEvent` 分开记录 pressed 和 repeat；
- `KeyModifiers(pub u8)` 用 `contains(flag)` 查询 Shift/Ctrl/Alt/Super/CapsLock；
- `InputEvent::Text(char)` 表示布局处理后的 Unicode scalar，不等于按键事件；
- `Tick(u64)` 是调用方注入的逻辑帧号，不是 wall clock。

窗口系统向业务层输出 `GuiEvent { window, widget, kind }`。`widget=None` 表示窗口级事件，
例如 close。`GuiEventKind` 当前包括点击、焦点、文本变化、提交、无符号值变化和关闭请求。
`CloseRequested` 只是请求，runtime 不会自动删除窗口。

已知类型缺口：`PointerEventKind::Scroll` 是 `i32`，但 `ValueChanged` 只有 `u32`，无法自然
表示负滚动。新增滚动控件应扩展有符号 delta 事件，而不是使用 `as u32`。

## 6. ID 和窗口/控件所有权

`WindowId(u64)` 和 `WidgetId(u64)` 是调用方分配、可复制/排序/hash 的稳定标识。API 不
提供全局 allocator，也不验证唯一性。软件实现用线性查找；重复 ID 会让更新、focus 和
事件路由产生歧义。

`Window` 拥有 `title: String` 和扁平 `Vec<Widget>`，不是递归 widget tree。`bounds` 是
屏幕坐标；widget bounds 相对窗口内容区。窗口默认 visible、movable、closable，active
初始为 false。

`Widget` 的公共字段是 ID、bounds、visible、enabled、kind。当前种类：

| kind | 拥有状态 | 默认构造行为 |
|---|---|---|
| `Panel` | background、可选 border | 仅 `Widget::new` |
| `Label` | `String`、`TextStyle` | 调用方提供 style |
| `Button` | text、renderer 维护的 pressed | pressed=false |
| `ProgressBar` | value、maximum、show_text | 构造器把 maximum 至少置 1，但不 clamp 初始 value |
| `TextInput` | text、placeholder、cursor、字符上限、password | 空文本、cursor=0、上限 256 |

visible/enabled 的具体绘制、命中和 focus 语义由实现层决定。直接替换 `kind` 或修改状态时，
调用者还必须通知 renderer 标脏；API 对象自身没有 observer。

## 7. TextInput 的 UTF-8 规则

`cursor` 是 `String` 的字节偏移，必须满足：

```rust
cursor <= text.len() && text.is_char_boundary(cursor)
```

`maximum_chars` 是 Unicode scalar 数量，不是字节数。插入一个 `char` 后 cursor 增加
`char.len_utf8()`；Backspace/Delete 必须按字符边界移动。API 的字段公开且不自动维护该
不变量，非法 cursor 可能使 renderer 的字符串 slice panic。

`InputEvent::Text(char)` 不承诺字体能显示该字符；模型可正确保存中文，而当前 5×7 字体
会显示问号。password 只是显示策略，不提供秘密内存擦除或侧信道防护。

## 8. 错误语义

| 错误 | 典型来源 |
|---|---|
| `NotInitialized` / `AlreadyInitialized` | 全局 runtime 生命周期 |
| `NoDisplay` | display index 不存在 |
| `InvalidSurface` | 尺寸、format、stride、buffer 长度 |
| `WindowNotFound` / `WidgetNotFound` | ID 查找或 kind 不匹配；当前部分实现统一返回 WidgetNotFound |
| `QueueFull` | bounded input/output 策略；当前 push_input 使用 |
| `DisplayFailure` | framebuffer/flush 驱动失败 |

`GuiError` 不携带底层错误细节，适合作稳定分类但不利于诊断。若扩展错误上下文，保持
无分配路径可用，并检查所有穷举 match。

## 9. 新增 WidgetKind 的端到端清单

以 Checkbox 为例：

1. 在本 crate 定义拥有状态并加入 `WidgetKind`；
2. 提供构造器，规范化范围、cursor 等不变量；
3. 决定它是否 focusable、是否 capture pointer、键盘 Space/Enter 行为；
4. 选择现有语义事件或新增精确事件，避免有符号/无符号丢失；
5. software scene 增加 render、hit/focus、状态转换和定向 dirty；
6. 全局实现增加 getter/setter，区分 WindowNotFound、WidgetNotFound、WrongKind；
7. 聚合 crate 再导出；如暴露 syscall，再加用户复制、长度限制、PID 所有权和退出清理；
8. 为 Clone/Eq、默认值、disabled/hidden、重复 ID 和队列满添加测试。

## 10. syscall 边界示例

不要把含 `String/Vec` 的 Rust 布局直接当用户 ABI。新增 `gui_set_text` 应定义固定布局：

```text
sys_gui_set_text(window_id, widget_id, user_ptr, byte_len)
  -> 检查 byte_len <= GUI_TEXT_MAX
  -> copy_from_user 到 fallible kernel buffer
  -> UTF-8 校验
  -> 检查 window 属于 current PID
  -> software_gui::set_label_text / 专用 text setter
  -> 标脏并映射 GuiError -> errno
```

必须明确 `EFAULT`、`EINVAL`、`ENOENT`、`EPERM`、`ENOMEM` 和长度超限，且绝不能在持 GUI
锁时 copy user memory 或等待 display。

## 11. 自回归

```sh
cargo test --manifest-path components/wateros-gui/gui-api/api-v0/Cargo.toml
cargo test --manifest-path components/wateros-gui/gui-impl/impl-software/Cargo.toml
python3 scripts/maintenance/check_offline_docs.py
make check ARCH=rv PROFILE=pre
make check ARCH=la PROFILE=pre
```

测试至少覆盖半开边界、空矩形、极值坐标、alpha 端点/BGRA 往返、ID 重复策略、progress
范围、TextInput 多字节 cursor、event 有符号值及新增 enum 的所有消费者。

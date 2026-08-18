//! 窗口与基础控件的拥有型数据结构。

use alloc::{string::String, vec::Vec};

use crate::{Color, Rect, TextStyle};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// 调用方分配的稳定窗口标识。
pub struct WindowId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// 窗口内部的稳定控件标识。
pub struct WidgetId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
/// 纯色容器，可选一像素边框。
pub struct Panel {
    /// 容器填充色。
    pub background : Color,
    /// 可选的一像素边框色；`None` 表示不绘制边框。
    pub border : Option<Color>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// 只读文本标签。
pub struct Label {
    pub text : String,
    pub style : TextStyle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// 可聚焦、可点击按钮；`pressed` 由场景状态机维护。
pub struct Button {
    /// 按钮显示文本。
    pub text : String,
    /// 指针按下但尚未释放时的视觉状态，由运行时维护。
    pub pressed : bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// 范围为 `0..=maximum` 的进度条。
pub struct ProgressBar {
    pub value : u32,
    pub maximum : u32,
    pub show_text : bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// 单行 UTF-8 编辑状态；当前内嵌字体只渲染可打印 ASCII。
pub struct TextInput {
    /// 当前 UTF-8 文本。
    pub text : String,
    /// 文本为空时的提示文本。
    pub placeholder : String,
    /// 插入点的字节下标；状态机必须维持其位于 UTF-8 字符边界。
    pub cursor : usize,
    /// 可输入的 Unicode 标量值最大数，而不是字节数。
    pub maximum_chars : usize,
    /// 是否以掩码而非明文绘制。
    pub password : bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// 当前软件 renderer 支持的控件集合。
pub enum WidgetKind {
    Panel(Panel),
    Label(Label),
    Button(Button),
    ProgressBar(ProgressBar),
    TextInput(TextInput),
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// 控件共有属性和控件专属状态。
pub struct Widget {
    pub id : WidgetId,
    /// 相对窗口内容区左上角的矩形。
    pub bounds : Rect,
    pub visible : bool,
    pub enabled : bool,
    pub kind : WidgetKind,
}

impl Widget {
    pub fn new(id : WidgetId, bounds : Rect, kind : WidgetKind) -> Self {
        Self { id,
               bounds,
               visible : true,
               enabled : true,
               kind }
    }

    pub fn label(id : WidgetId, bounds : Rect, text : impl Into<String>, style : TextStyle) -> Self {
        Self::new(id, bounds, WidgetKind::Label(Label { text : text.into(), style }))
    }

    pub fn button(id : WidgetId, bounds : Rect, text : impl Into<String>) -> Self {
        Self::new(id,
                  bounds,
                  WidgetKind::Button(Button { text : text.into(), pressed : false }))
    }

   pub fn progress(id : WidgetId, bounds : Rect, value : u32, maximum : u32) -> Self {
        // 最大值钳制为一，避免后续绘制百分比时除零。
        Self::new(id,
                  bounds,
                  WidgetKind::ProgressBar(ProgressBar { value,
                                                        maximum : maximum.max(1),
                                                        show_text : true }))
    }

    pub fn text_input(id : WidgetId, bounds : Rect, placeholder : impl Into<String>) -> Self {
        Self::new(id,
                  bounds,
                  WidgetKind::TextInput(TextInput { text : String::new(),
                                                    placeholder : placeholder.into(),
                                                    cursor : 0,
                                                    maximum_chars : 256,
                                                    password : false }))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// 拥有控件列表的顶层窗口。`bounds` 使用屏幕坐标。
pub struct Window {
    pub id : WindowId,
    pub title : String,
    pub bounds : Rect,
    pub visible : bool,
    pub active : bool,
    pub movable : bool,
    pub closable : bool,
    pub background : Color,
    pub widgets : Vec<Widget>,
}

impl Window {
    pub fn new(id : WindowId, title : impl Into<String>, bounds : Rect) -> Self {
        Self { id,
               title : title.into(),
               bounds,
               visible : true,
               active : false,
               movable : true,
               closable : true,
               background : Color::rgb(22, 34, 54),
               widgets : Vec::new() }
    }

    pub fn add_widget(&mut self, widget : Widget) { self.widgets.push(widget); }
}

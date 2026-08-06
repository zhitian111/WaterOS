//! GUI 输入事件与语义事件。

use crate::{Point, WidgetId, WindowId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// 指针设备按钮；未知硬件按钮保留原始 evdev 编码。
pub enum PointerButton {
    Left,
    Middle,
    Right,
    Other(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// 已经换算为屏幕坐标的指针动作。
pub enum PointerEventKind {
    Move,
    Button { button : PointerButton, pressed : bool },
    Scroll { horizontal : i32, vertical : i32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// 指针当前位置与本次动作。
pub struct PointerEvent {
    pub position : Point,
    pub kind : PointerEventKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// 与具体硬件扫描码解耦的逻辑键。
pub enum KeyCode {
    Unknown(u16),
    Escape,
    Enter,
    Tab,
    Backspace,
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Left,
    Right,
    Up,
    Down,
    Space,
    Function(u8),
    Character(char),
}

/// Shift/Ctrl/Alt/Super 和锁定键的紧凑位图。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
/// 修饰键状态位图；常量值可用 [`KeyModifiers::contains`] 查询。
pub struct KeyModifiers(pub u8);

impl KeyModifiers {
    pub const SHIFT : u8 = 1 << 0;
    pub const CTRL : u8 = 1 << 1;
    pub const ALT : u8 = 1 << 2;
    pub const SUPER : u8 = 1 << 3;
    pub const CAPS_LOCK : u8 = 1 << 4;

    pub const fn contains(self, flag : u8) -> bool { self.0 & flag != 0 }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// 单次逻辑键按下、释放或自动重复。
pub struct KeyEvent {
    pub code : KeyCode,
    pub modifiers : KeyModifiers,
    pub pressed : bool,
    pub repeat : bool,
}

/// 硬件输入后端向 GUI 注入的统一事件。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// 输入后端与窗口系统之间的统一事件。
pub enum InputEvent {
    Pointer(PointerEvent),
    Key(KeyEvent),
    /// 已经完成键盘布局/组合键处理的 Unicode 字符。
    Text(char),
    /// GUI 周期任务产生的逻辑帧号。
    Tick(u64),
}

/// 窗口和控件向调用方上报的语义事件。
#[derive(Debug, Clone, PartialEq, Eq)]
/// 控件状态机向业务层报告的动作，不携带硬件细节。
pub enum GuiEventKind {
    Clicked,
    FocusGained,
    FocusLost,
    TextChanged,
    Submitted,
    ValueChanged(u32),
    CloseRequested,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// 由窗口/控件 ID 定位来源的业务语义事件。
pub struct GuiEvent {
    pub window : WindowId,
    pub widget : Option<WidgetId>,
    pub kind : GuiEventKind,
}

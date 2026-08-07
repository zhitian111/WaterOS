//! WaterOS GUI 的版本化公共数据模型。
//!
//! 本 crate 只描述几何、颜色、文本、输入事件、窗口和控件，不依赖 GPU、
//! 调度器或某个具体渲染器。硬件后端和软件实现因此可以分别演进。

#![no_std]
extern crate alloc;

mod color;
mod event;
mod geometry;
mod text;
mod widget;

pub use color::Color;
pub use event::{
    GuiEvent, GuiEventKind, InputEvent, KeyCode, KeyEvent, KeyModifiers, PointerButton,
    PointerEvent, PointerEventKind,
};
pub use geometry::{Insets, Point, Rect, Size};
pub use text::{TextAlign, TextMetrics, TextStyle, TextWrap, VerticalAlign};
pub use widget::{
    Button, Label, Panel, ProgressBar, TextInput, Widget, WidgetId, WidgetKind, Window,
    WindowId,
};

/// GUI 操作的稳定错误分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuiError {
    /// GUI 尚未完成初始化。
    NotInitialized,
    /// 已经存在一个全局 GUI 实例。
    AlreadyInitialized,
    /// 没有可用显示设备。
    NoDisplay,
    /// framebuffer 尺寸、步长或缓冲长度无效。
    InvalidSurface,
    /// 指定窗口不存在。
    WindowNotFound,
    /// 指定控件不存在。
    WidgetNotFound,
    /// 输入或输出事件队列已满。
    QueueFull,
    /// 底层显示驱动操作失败。
    DisplayFailure,
}

/// GUI API 的统一返回类型。
pub type GuiResult<T> = core::result::Result<T, GuiError>;

//! 默认启动桌面。它只使用公开窗口/控件 API，可由其它场景完全替换。

use alloc::format;
use api_v0::{
    Color, GuiResult, Rect, TextStyle, TextWrap, VerticalAlign, Widget, WidgetId, Window,
    WindowId,
};

use crate::{add_window, runtime_snapshot, set_label_text, set_progress};

pub const MAIN_WINDOW : WindowId = WindowId(1);
pub const SYSTEM_WINDOW : WindowId = WindowId(2);
pub const STATUS_LABEL : WidgetId = WidgetId(101);
pub const PROGRESS : WidgetId = WidgetId(102);
pub const ACTION_BUTTON : WidgetId = WidgetId(103);
pub const COMMAND_INPUT : WidgetId = WidgetId(104);
pub const SYSTEM_LABEL : WidgetId = WidgetId(201);

pub fn install_default_desktop() -> GuiResult<()> {
    let size = runtime_snapshot()?.size;
    let margin = (size.width / 24).clamp(16, 48);
    let main_width = (size.width * 2 / 3).max(360).min(size.width.saturating_sub(margin * 2));
    let main_height = (size.height * 2 / 3).max(260).min(size.height.saturating_sub(margin * 2));
    let mut main = Window::new(MAIN_WINDOW,
                               "WaterOS GUI",
                               Rect::new(margin as i32,
                                         margin as i32,
                                         main_width,
                                         main_height));
    main.background = Color::rgb(17, 29, 48);
    main.add_widget(Widget::label(WidgetId(100),
                                  Rect::new(24, 20, main_width.saturating_sub(48), 52),
                                  "WaterOS graphical runtime",
                                  TextStyle { foreground : Color::rgb(239, 247, 255),
                                              scale : 3,
                                              vertical : VerticalAlign::Middle,
                                              ..TextStyle::default() }));
    main.add_widget(Widget::label(STATUS_LABEL,
                                  Rect::new(24, 82, main_width.saturating_sub(48), 54),
                                  "VirtIO GPU / shadow buffer / compositor ready",
                                  TextStyle { foreground : Color::rgb(154, 185, 221),
                                              scale : 2,
                                              wrap : TextWrap::Word,
                                              ..TextStyle::default() }));
    main.add_widget(Widget::progress(PROGRESS,
                                     Rect::new(24, 150, main_width.saturating_sub(48), 28),
                                     0,
                                     100));
    main.add_widget(Widget::text_input(COMMAND_INPUT,
                                       Rect::new(24, 198, main_width.saturating_sub(48), 38),
                                       "Type here after an input backend is connected"));
    main.add_widget(Widget::button(ACTION_BUTTON,
                                   Rect::new(24, 252, 180, 42),
                                   "Run self-check"));
    add_window(main)?;

    if size.width >= 820 && size.height >= 480 {
        let width = 280;
        let height = 190;
        let mut system = Window::new(SYSTEM_WINDOW,
                                     "System",
                                     Rect::new((size.width - width - margin) as i32,
                                               (size.height - height - margin) as i32,
                                               width,
                                               height));
        system.closable = false;
        system.add_widget(Widget::label(SYSTEM_LABEL,
                                        Rect::new(16, 16, width - 32, height - 48),
                                        format!("Display: {}x{}\nPixel: BGRA8888\nWindows: 2\nInput queue: ready",
                                                size.width, size.height),
                                        TextStyle { foreground : Color::rgb(194, 215, 238),
                                                    scale : 2,
                                                    wrap : TextWrap::Word,
                                                    ..TextStyle::default() }));
        add_window(system)?;
    }
    Ok(())
}

/// 更新默认桌面的动画信息。调用方可以按任意频率传入单调递增帧号。
pub fn update_default_desktop(frame : u64) -> GuiResult<()> {
    let progress = (frame % 101) as u32;
    let _ = set_progress(MAIN_WINDOW, PROGRESS, progress);
    if frame % 25 == 0 {
        let snapshot = runtime_snapshot()?;
        let _ = set_label_text(MAIN_WINDOW,
                               STATUS_LABEL,
                               format!("Frame {} | presented {} | events {} | dropped {}",
                                       frame,
                                       snapshot.frames_presented,
                                       snapshot.pending_input,
                                       snapshot.dropped_input));
    }
    Ok(())
}

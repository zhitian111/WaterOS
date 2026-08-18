//! 窗口树、命中测试、焦点管理和控件事件状态机。

use alloc::{collections::VecDeque, format, string::String, vec::Vec};
use api_v0::{
    Color, GuiEvent, GuiEventKind, InputEvent, KeyCode, Point, PointerButton, PointerEvent,
    PointerEventKind, Rect, TextAlign, TextStyle, VerticalAlign, Widget, WidgetId,
    WidgetKind, Window, WindowId,
};

use crate::{Canvas, Theme};

pub const TITLE_BAR_HEIGHT : u32 = 28;
const WINDOW_BORDER : u32 = 2;
const CLOSE_SIZE : u32 = 18;
const CURSOR_RADIUS : u32 = 5;

#[derive(Clone, Copy, PartialEq, Eq)]
enum HitTarget {
    Desktop,
    Window(WindowId),
    TitleBar(WindowId),
    Close(WindowId),
    Widget(WindowId, WidgetId),
}

struct WindowDrag {
    window : WindowId,
    offset : Point,
}

/// 实现层持有的桌面状态。窗口 Vec 的顺序就是从底到顶的 z 序。
pub struct Desktop {
    /// 从底到顶排列的窗口；最后一个可视窗口通常是最上层。
    windows : Vec<Window>,
    /// 最近一次指针位置，单位为屏幕像素。
    pointer : Point,
    /// 当前键盘焦点控件；窗口被删除时必须清除。
    focused : Option<(WindowId, WidgetId)>,
    /// 鼠标按下后捕获的控件，保证拖动/释放跨出控件边界仍归原控件处理。
    captured : Option<(WindowId, WidgetId)>,
    /// 当前窗口拖动状态及按下点相对窗口原点的偏移。
    drag : Option<WindowDrag>,
    /// 文本输入插入符是否可见。
    caret_visible : bool,
    /// 上次切换插入符可见性的逻辑帧相位。
    last_caret_phase : u64,
}

impl Desktop {
    pub fn new() -> Self {
        Self { windows : Vec::new(),
               pointer : Point::new(32, 32),
               focused : None,
               captured : None,
               drag : None,
               caret_visible : true,
               last_caret_phase : 0 }
    }

    pub fn windows(&self) -> &[Window] { &self.windows }

    pub fn add_window(&mut self, mut window : Window) {
        // 新窗口置顶并取消旧窗口 active，保证 z 序和标题栏状态一致。
        for existing in &mut self.windows {
            existing.active = false;
        }
        window.active = true;
        self.windows.push(window);
    }

    pub fn remove_window(&mut self, id : WindowId) -> bool {
        let Some(index) = self.windows.iter().position(|window| window.id == id) else {
            return false;
        };
        self.windows.remove(index);
        if self.focused.is_some_and(|(window, _)| window == id) {
            self.focused = None;
        }
        if let Some(top) = self.windows.last_mut() {
            top.active = true;
        }
        true
    }

    pub fn set_label_text(&mut self, window : WindowId, widget : WidgetId, text : String)
                          -> Result<Option<Rect>, ()> {
        let (window_bounds, control) = self.find_widget_mut(window, widget).ok_or(())?;
        let WidgetKind::Label(label) = &mut control.kind else { return Err(()) };
        if label.text == text {
            return Ok(None);
        }
        label.text = text;
        Ok(Some(widget_screen_rect(window_bounds, control.bounds)))
    }

    pub fn set_progress(&mut self, window : WindowId, widget : WidgetId, value : u32)
                        -> Result<Option<Rect>, ()> {
        let (window_bounds, control) = self.find_widget_mut(window, widget).ok_or(())?;
        let WidgetKind::ProgressBar(progress) = &mut control.kind else { return Err(()) };
        // 外部值可能超过最大值；钳制后再比较，避免进度条绘制比例溢出。
        let value = value.min(progress.maximum);
        if progress.value == value {
            return Ok(None);
        }
        progress.value = value;
        Ok(Some(widget_screen_rect(window_bounds, control.bounds)))
    }

    /// 处理一个输入事件，返回是否改变了画面。
    pub fn handle_input(&mut self, input : InputEvent, output : &mut VecDeque<GuiEvent>) -> bool {
        // 释放键只用于状态机清理，不重复触发按下语义；Tick 仅按固定帧相位闪烁插入符。
        match input {
            InputEvent::Pointer(event) => self.handle_pointer(event, output),
            InputEvent::Key(event) if event.pressed => self.handle_key(event.code, output),
            InputEvent::Text(character) => self.insert_text(character, output),
            InputEvent::Tick(frame) => {
                let phase = frame / 30;
                if phase != self.last_caret_phase {
                    self.last_caret_phase = phase;
                    self.caret_visible = phase % 2 == 0;
                    self.focused.is_some()
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    pub fn render(&self, canvas : &mut Canvas<'_>, theme : &Theme) {
        render_desktop_background(canvas, theme);
        for window in self.windows.iter().filter(|window| window.visible) {
            self.render_window(canvas, theme, window);
        }
        render_pointer(canvas, self.pointer, theme);
    }

    fn render_window(&self, canvas : &mut Canvas<'_>, theme : &Theme, window : &Window) {
        let bounds = window.bounds;
        let shadow = Rect::new(bounds.origin.x + 5,
                               bounds.origin.y + 6,
                               bounds.size.width,
                               bounds.size.height);
        canvas.fill_rect(shadow, Color::rgba(0, 0, 0, 100));
        canvas.fill_rect(bounds, window.background);
        canvas.stroke_rect(bounds, WINDOW_BORDER, theme.window_border);

        let title = title_bar_rect(bounds);
        canvas.fill_rect(title,
                         if window.active { theme.title_active } else { theme.title_inactive });
        let title_style = TextStyle { foreground : theme.text,
                                      scale : 2,
                                      vertical : VerticalAlign::Middle,
                                      ..TextStyle::default() };
        canvas.draw_text(Rect::new(title.origin.x + 10,
                                   title.origin.y,
                                   title.size.width.saturating_sub(42),
                                   title.size.height),
                         &window.title,
                         title_style);
        if window.closable {
            let close = close_rect(bounds);
            canvas.fill_rect(close, theme.danger);
            canvas.draw_line(Point::new(close.origin.x + 5, close.origin.y + 5),
                             Point::new(close.right() - 6, close.bottom() - 6),
                             theme.text);
            canvas.draw_line(Point::new(close.right() - 6, close.origin.y + 5),
                             Point::new(close.origin.x + 5, close.bottom() - 6),
                             theme.text);
        }

        let content = content_rect(bounds);
        let old_clip = canvas.set_clip(content);
        for widget in window.widgets.iter().filter(|widget| widget.visible) {
            self.render_widget(canvas, theme, window, widget);
        }
        canvas.restore_clip(old_clip);
    }

    fn render_widget(&self,
                     canvas : &mut Canvas<'_>,
                     theme : &Theme,
                     window : &Window,
                     widget : &Widget) {
        let bounds = widget_screen_rect(window.bounds, widget.bounds);
        let hovered = self.hit_test(self.pointer) == HitTarget::Widget(window.id, widget.id);
        let focused = self.focused == Some((window.id, widget.id));
        match &widget.kind {
            WidgetKind::Panel(panel) => {
                canvas.fill_rect(bounds, panel.background);
                if let Some(border) = panel.border {
                    canvas.stroke_rect(bounds, 1, border);
                }
            }
            WidgetKind::Label(label) => {
                canvas.draw_text(bounds, &label.text, label.style);
            }
            WidgetKind::Button(button) => {
                let background = if button.pressed {
                    theme.control_pressed
                } else if hovered {
                    theme.control_hover
                } else {
                    theme.control
                };
                canvas.fill_rect(bounds, background);
                canvas.stroke_rect(bounds, if focused { 2 } else { 1 },
                                   if focused { theme.focus } else { theme.window_border });
                canvas.draw_text(bounds.inset(api_v0::Insets::uniform(4)),
                                 &button.text,
                                 TextStyle { foreground : if widget.enabled {
                                                 theme.text
                                             } else {
                                                 theme.text_muted
                                             },
                                             scale : 2,
                                             horizontal : TextAlign::Center,
                                             vertical : VerticalAlign::Middle,
                                             ..TextStyle::default() });
            }
            WidgetKind::ProgressBar(progress) => {
                canvas.fill_rect(bounds, theme.progress_track);
                let filled = (u64::from(bounds.size.width) * u64::from(progress.value) /
                              u64::from(progress.maximum.max(1))) as u32;
                canvas.fill_rect(Rect::new(bounds.origin.x,
                                           bounds.origin.y,
                                           filled,
                                           bounds.size.height),
                                 theme.progress_fill);
                canvas.stroke_rect(bounds, 1, theme.window_border);
                if progress.show_text {
                    let percent = progress.value.saturating_mul(100) / progress.maximum.max(1);
                    canvas.draw_text(bounds,
                                     &format!("{}%", percent),
                                     TextStyle { foreground : theme.text,
                                                 scale : 1,
                                                 horizontal : TextAlign::Center,
                                                 vertical : VerticalAlign::Middle,
                                                 ..TextStyle::default() });
                }
            }
            WidgetKind::TextInput(input) => {
                canvas.fill_rect(bounds, Color::rgb(8, 17, 29));
                canvas.stroke_rect(bounds, if focused { 2 } else { 1 },
                                   if focused { theme.focus } else { theme.window_border });
                let display = if input.text.is_empty() {
                    input.placeholder.clone()
                } else if input.password {
                    "*".repeat(input.text.chars().count())
                } else {
                    input.text.clone()
                };
                let foreground = if input.text.is_empty() { theme.text_muted } else { theme.text };
                let text_bounds = bounds.inset(api_v0::Insets { left : 7,
                                                                top : 4,
                                                                right : 7,
                                                                bottom : 4 });
                canvas.draw_text(text_bounds,
                                 &display,
                                 TextStyle { foreground,
                                             scale : 2,
                                             vertical : VerticalAlign::Middle,
                                             ..TextStyle::default() });
                if focused && self.caret_visible {
                    let cursor_chars = input.text[..input.cursor.min(input.text.len())].chars().count();
                    let caret_x = text_bounds.origin.x + (cursor_chars as u32 * 12) as i32;
                    canvas.fill_rect(Rect::new(caret_x,
                                               text_bounds.origin.y + 2,
                                               2,
                                               text_bounds.size.height.saturating_sub(4)),
                                     theme.focus);
                }
            }
        }
    }

    fn handle_pointer(&mut self, event : PointerEvent, output : &mut VecDeque<GuiEvent>) -> bool {
        let old_pointer = self.pointer;
        self.pointer = event.position;
        match event.kind {
            PointerEventKind::Move => {
                if let Some(drag) = &self.drag {
                    if let Some(window) = self.windows.iter_mut().find(|window| window.id == drag.window) {
                        window.bounds.origin = Point::new(event.position.x - drag.offset.x,
                                                          event.position.y - drag.offset.y);
                    }
                }
                old_pointer != self.pointer || self.drag.is_some()
            }
            PointerEventKind::Button { button : PointerButton::Left, pressed : true } => {
                self.pointer_down(output)
            }
            PointerEventKind::Button { button : PointerButton::Left, pressed : false } => {
                self.pointer_up(output)
            }
            PointerEventKind::Scroll { vertical, .. } => {
                if let Some((window, widget)) = self.focused {
                    output.push_back(GuiEvent { window,
                                                widget : Some(widget),
                                                kind : GuiEventKind::ValueChanged(vertical as u32) });
                }
                false
            }
            _ => false,
        }
    }

    fn pointer_down(&mut self, output : &mut VecDeque<GuiEvent>) -> bool {
        let target = self.hit_test(self.pointer);
        let window_id = match target {
            HitTarget::Window(id) | HitTarget::TitleBar(id) | HitTarget::Close(id) |
            HitTarget::Widget(id, _) => Some(id),
            HitTarget::Desktop => None,
        };
        if let Some(id) = window_id {
            self.bring_to_front(id);
        }
        match target {
            HitTarget::Desktop => self.change_focus(None, output),
            HitTarget::Window(_) => self.change_focus(None, output) || true,
            HitTarget::Close(window) => {
                output.push_back(GuiEvent { window,
                                            widget : None,
                                            kind : GuiEventKind::CloseRequested });
                true
            }
            HitTarget::TitleBar(window) => {
                if let Some(target) = self.windows.iter().find(|candidate| candidate.id == window) {
                    if target.movable {
                        self.drag = Some(WindowDrag { window,
                                                     offset : Point::new(self.pointer.x -
                                                                         target.bounds.origin.x,
                                                                         self.pointer.y -
                                                                         target.bounds.origin.y) });
                    }
                }
                self.change_focus(None, output) || true
            }
            HitTarget::Widget(window, widget) => {
                let mut changed = self.change_focus(Some((window, widget)), output);
                self.captured = Some((window, widget));
                if let Some((_, control)) = self.find_widget_mut(window, widget) {
                    if let WidgetKind::Button(button) = &mut control.kind {
                        button.pressed = true;
                        changed = true;
                    }
                }
                changed
            }
        }
    }

    fn pointer_up(&mut self, output : &mut VecDeque<GuiEvent>) -> bool {
        self.drag = None;
        let Some((window, widget)) = self.captured.take() else { return false };
        let still_inside = self.hit_test(self.pointer) == HitTarget::Widget(window, widget);
        let mut was_button = false;
        if let Some((_, control)) = self.find_widget_mut(window, widget) {
            if let WidgetKind::Button(button) = &mut control.kind {
                was_button = true;
                button.pressed = false;
            }
        }
        if was_button && still_inside {
            output.push_back(GuiEvent { window,
                                        widget : Some(widget),
                                        kind : GuiEventKind::Clicked });
        }
        was_button
    }

    fn handle_key(&mut self, code : KeyCode, output : &mut VecDeque<GuiEvent>) -> bool {
        match code {
            KeyCode::Tab => self.focus_next(output),
            KeyCode::Backspace => self.delete_before_cursor(output),
            KeyCode::Delete => self.delete_at_cursor(output),
            KeyCode::Left => self.move_cursor(false),
            KeyCode::Right => self.move_cursor(true),
            KeyCode::Home => self.set_cursor_edge(false),
            KeyCode::End => self.set_cursor_edge(true),
            KeyCode::Enter => {
                if let Some((window, widget)) = self.focused {
                    output.push_back(GuiEvent { window,
                                                widget : Some(widget),
                                                kind : GuiEventKind::Submitted });
                    true
                } else {
                    false
                }
            }
            KeyCode::Space => self.activate_focused_button(output),
            _ => false,
        }
    }

    fn insert_text(&mut self, character : char, output : &mut VecDeque<GuiEvent>) -> bool {
        if character.is_control() {
            return false;
        }
        let Some((window, widget)) = self.focused else { return false };
        let Some((_, control)) = self.find_widget_mut(window, widget) else { return false };
        let WidgetKind::TextInput(input) = &mut control.kind else { return false };
        if input.text.chars().count() >= input.maximum_chars {
            return false;
        }
        input.cursor = input.cursor.min(input.text.len());
        while !input.text.is_char_boundary(input.cursor) {
            input.cursor -= 1;
        }
        input.text.insert(input.cursor, character);
        input.cursor += character.len_utf8();
        output.push_back(GuiEvent { window,
                                    widget : Some(widget),
                                    kind : GuiEventKind::TextChanged });
        true
    }

    fn delete_before_cursor(&mut self, output : &mut VecDeque<GuiEvent>) -> bool {
        let Some((window, widget)) = self.focused else { return false };
        let Some((_, control)) = self.find_widget_mut(window, widget) else { return false };
        let WidgetKind::TextInput(input) = &mut control.kind else { return false };
        if input.cursor == 0 || input.text.is_empty() {
            return false;
        }
        let previous = input.text[..input.cursor.min(input.text.len())]
                            .char_indices().last().map(|(index, _)| index).unwrap_or(0);
        input.text.drain(previous..input.cursor);
        input.cursor = previous;
        output.push_back(GuiEvent { window,
                                    widget : Some(widget),
                                    kind : GuiEventKind::TextChanged });
        true
    }

    fn delete_at_cursor(&mut self, output : &mut VecDeque<GuiEvent>) -> bool {
        let Some((window, widget)) = self.focused else { return false };
        let Some((_, control)) = self.find_widget_mut(window, widget) else { return false };
        let WidgetKind::TextInput(input) = &mut control.kind else { return false };
        if input.cursor >= input.text.len() {
            return false;
        }
        let length = input.text[input.cursor..].chars().next().map(char::len_utf8).unwrap_or(0);
        input.text.drain(input.cursor..input.cursor + length);
        output.push_back(GuiEvent { window,
                                    widget : Some(widget),
                                    kind : GuiEventKind::TextChanged });
        true
    }

    fn move_cursor(&mut self, forward : bool) -> bool {
        let Some((window, widget)) = self.focused else { return false };
        let Some((_, control)) = self.find_widget_mut(window, widget) else { return false };
        let WidgetKind::TextInput(input) = &mut control.kind else { return false };
        let old = input.cursor;
        if forward {
            input.cursor += input.text[input.cursor.min(input.text.len())..]
                                      .chars().next().map(char::len_utf8).unwrap_or(0);
        } else if input.cursor > 0 {
            input.cursor = input.text[..input.cursor]
                                .char_indices().last().map(|(index, _)| index).unwrap_or(0);
        }
        old != input.cursor
    }

    fn set_cursor_edge(&mut self, end : bool) -> bool {
        let Some((window, widget)) = self.focused else { return false };
        let Some((_, control)) = self.find_widget_mut(window, widget) else { return false };
        let WidgetKind::TextInput(input) = &mut control.kind else { return false };
        let target = if end { input.text.len() } else { 0 };
        let changed = input.cursor != target;
        input.cursor = target;
        changed
    }

    fn activate_focused_button(&mut self, output : &mut VecDeque<GuiEvent>) -> bool {
        let Some((window, widget)) = self.focused else { return false };
        let Some((_, control)) = self.find_widget_mut(window, widget) else { return false };
        if !matches!(control.kind, WidgetKind::Button(_)) {
            return false;
        }
        output.push_back(GuiEvent { window,
                                    widget : Some(widget),
                                    kind : GuiEventKind::Clicked });
        true
    }

    fn focus_next(&mut self, output : &mut VecDeque<GuiEvent>) -> bool {
        let focusable : Vec<(WindowId, WidgetId)> = self.windows.iter().rev()
            .flat_map(|window| window.widgets.iter().filter_map(move |widget| {
                (window.visible && widget.visible && widget.enabled &&
                 matches!(widget.kind, WidgetKind::Button(_) | WidgetKind::TextInput(_)))
                    .then_some((window.id, widget.id))
            }))
            .collect();
        if focusable.is_empty() {
            return self.change_focus(None, output);
        }
        let next = self.focused.and_then(|focused| focusable.iter().position(|entry| *entry == focused))
                               .map(|index| focusable[(index + 1) % focusable.len()])
                               .unwrap_or(focusable[0]);
        self.change_focus(Some(next), output)
    }

    fn change_focus(&mut self,
                    next : Option<(WindowId, WidgetId)>,
                    output : &mut VecDeque<GuiEvent>)
                    -> bool {
        if self.focused == next {
            return false;
        }
        if let Some((window, widget)) = self.focused.take() {
            output.push_back(GuiEvent { window,
                                        widget : Some(widget),
                                        kind : GuiEventKind::FocusLost });
        }
        self.focused = next;
        if let Some((window, widget)) = next {
            output.push_back(GuiEvent { window,
                                        widget : Some(widget),
                                        kind : GuiEventKind::FocusGained });
        }
        self.caret_visible = true;
        true
    }

    fn bring_to_front(&mut self, id : WindowId) {
        let Some(index) = self.windows.iter().position(|window| window.id == id) else { return };
        let mut window = self.windows.remove(index);
        for existing in &mut self.windows {
            existing.active = false;
        }
        window.active = true;
        self.windows.push(window);
    }

    fn hit_test(&self, point : Point) -> HitTarget {
        for window in self.windows.iter().rev().filter(|window| window.visible) {
            if !window.bounds.contains(point) {
                continue;
            }
            if window.closable && close_rect(window.bounds).contains(point) {
                return HitTarget::Close(window.id);
            }
            if title_bar_rect(window.bounds).contains(point) {
                return HitTarget::TitleBar(window.id);
            }
            for widget in window.widgets.iter().rev().filter(|widget| widget.visible && widget.enabled) {
                if widget_screen_rect(window.bounds, widget.bounds).contains(point) {
                    return HitTarget::Widget(window.id, widget.id);
                }
            }
            return HitTarget::Window(window.id);
        }
        HitTarget::Desktop
    }

    fn find_widget_mut(&mut self, window : WindowId, widget : WidgetId) -> Option<(Rect, &mut Widget)> {
        let window = self.windows.iter_mut().find(|candidate| candidate.id == window)?;
        let bounds = window.bounds;
        let widget = window.widgets.iter_mut().find(|candidate| candidate.id == widget)?;
        Some((bounds, widget))
    }
}

fn render_desktop_background(canvas : &mut Canvas<'_>, theme : &Theme) {
    canvas.clear(theme.desktop);
    let bounds = canvas.bounds();
    for y in (0..bounds.size.height).step_by(48) {
        canvas.draw_line(Point::new(0, y as i32),
                         Point::new(bounds.right() - 1, y as i32),
                         theme.desktop_accent);
    }
    for x in (0..bounds.size.width).step_by(48) {
        canvas.draw_line(Point::new(x as i32, 0),
                         Point::new(x as i32, bounds.bottom() - 1),
                         theme.desktop_accent);
    }
}

fn render_pointer(canvas : &mut Canvas<'_>, point : Point, theme : &Theme) {
    canvas.fill_circle(point, CURSOR_RADIUS, Color::rgba(0, 0, 0, 130));
    canvas.draw_line(point, Point::new(point.x + 12, point.y + 18), theme.text);
    canvas.draw_line(point, Point::new(point.x, point.y + 20), theme.text);
    canvas.draw_line(Point::new(point.x, point.y + 20),
                     Point::new(point.x + 5, point.y + 15),
                     theme.text);
}

fn title_bar_rect(window : Rect) -> Rect {
    Rect::new(window.origin.x, window.origin.y, window.size.width, TITLE_BAR_HEIGHT.min(window.size.height))
}

fn content_rect(window : Rect) -> Rect {
    Rect::new(window.origin.x + WINDOW_BORDER as i32,
              window.origin.y + TITLE_BAR_HEIGHT as i32,
              window.size.width.saturating_sub(WINDOW_BORDER * 2),
              window.size.height.saturating_sub(TITLE_BAR_HEIGHT + WINDOW_BORDER))
}

fn close_rect(window : Rect) -> Rect {
    Rect::new(window.right() - CLOSE_SIZE as i32 - 6,
              window.origin.y + 5,
              CLOSE_SIZE,
              CLOSE_SIZE)
}

fn widget_screen_rect(window : Rect, widget : Rect) -> Rect {
    widget.translated(window.origin.x + WINDOW_BORDER as i32,
                      window.origin.y + TITLE_BAR_HEIGHT as i32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use api_v0::{PointerEvent, Size, Widget};

    #[test]
    fn button_click_is_routed_to_top_window() {
        let mut desktop = Desktop::new();
        let mut window = Window::new(WindowId(1), "test", Rect::new(10, 10, 200, 120));
        window.add_widget(Widget::button(WidgetId(2), Rect::new(10, 10, 80, 30), "OK"));
        desktop.add_window(window);
        let point = Point::new(25, 55);
        let mut output = VecDeque::new();
        desktop.handle_input(InputEvent::Pointer(PointerEvent {
            position : point,
            kind : PointerEventKind::Button { button : PointerButton::Left, pressed : true },
        }), &mut output);
        desktop.handle_input(InputEvent::Pointer(PointerEvent {
            position : point,
            kind : PointerEventKind::Button { button : PointerButton::Left, pressed : false },
        }), &mut output);
        assert!(output.iter().any(|event| event.kind == GuiEventKind::Clicked));
        let _ = Size::new(1, 1);
    }
}

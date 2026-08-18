//! 全局 GUI 实例的薄封装。

use alloc::string::String;
use api_v0::{GuiError, GuiEvent, GuiResult, InputEvent, WidgetId, Window, WindowId};
use spin::Mutex;

use crate::{GuiRuntime, GuiRuntimeSnapshot, Theme};

/// 全局 GUI 运行时；锁保护初始化、窗口树、事件队列与 shadow surface 的整体一致性。
/// 持锁期间不可阻塞、调度或重入本模块的公开函数。
static RUNTIME : Mutex<Option<GuiRuntime>> = Mutex::new(None);

/// 在默认显示设备（索引 0）上初始化 GUI；已初始化或无显示设备时返回错误。
pub fn initialize() -> GuiResult<()> {
    initialize_on(0)
}

/// 在指定显示设备上建立 GUI；为多显示器/测试后端保留稳定入口。
pub fn initialize_on(display_index : usize) -> GuiResult<()> {
    let mut slot = RUNTIME.lock();
    if slot.is_some() {
        return Err(GuiError::AlreadyInitialized);
    }
    let display = display::display_device_at(display_index).ok_or(GuiError::NoDisplay)?;
    *slot = Some(GuiRuntime::new(display)?);
    Ok(())
}

/// 释放 shadow surface、窗口树和设备引用。返回此前是否已经初始化。
pub fn shutdown() -> bool { RUNTIME.lock().take().is_some() }

pub fn is_initialized() -> bool { RUNTIME.lock().is_some() }

/// 在 GUI 全局锁内执行短操作。回调不得调度、等待或获取 display 锁。
pub fn with_runtime<R>(operation : impl FnOnce(&mut GuiRuntime) -> R) -> GuiResult<R> {
    let mut slot = RUNTIME.lock();
    let runtime = slot.as_mut().ok_or(GuiError::NotInitialized)?;
    Ok(operation(runtime))
}

pub fn add_window(window : Window) -> GuiResult<()> { with_runtime(|runtime| runtime.add_window(window)) }

pub fn remove_window(window : WindowId) -> GuiResult<bool> {
    with_runtime(|runtime| runtime.remove_window(window))
}

pub fn set_label_text(window : WindowId, widget : WidgetId, text : impl Into<String>)
                      -> GuiResult<()> {
    with_runtime(|runtime| runtime.set_label_text(window, widget, text.into()))?
}

pub fn set_progress(window : WindowId, widget : WidgetId, value : u32) -> GuiResult<()> {
    with_runtime(|runtime| runtime.set_progress(window, widget, value))?
}

pub fn set_theme(theme : Theme) -> GuiResult<()> {
    with_runtime(|runtime| runtime.set_theme(theme))
}

pub fn mark_dirty(region : api_v0::Rect) -> GuiResult<()> {
    with_runtime(|runtime| runtime.mark_dirty(region))
}

pub fn push_input(event : InputEvent) -> GuiResult<()> {
    with_runtime(|runtime| runtime.push_input(event))?
}

pub fn process_pending_input() -> GuiResult<usize> {
    with_runtime(GuiRuntime::process_pending_input)
}

/// 非阻塞轮询 virtio-input 等硬件输入源。
pub fn poll_hardware_input() -> GuiResult<usize> {
    with_runtime(GuiRuntime::poll_hardware_input)
}

pub fn poll_event() -> GuiResult<Option<GuiEvent>> { with_runtime(GuiRuntime::poll_event) }

pub fn render() -> GuiResult<bool> { with_runtime(GuiRuntime::render)? }

pub fn render_if_dirty() -> GuiResult<bool> {
    with_runtime(|runtime| {
        runtime.poll_hardware_input();
        runtime.process_pending_input();
        runtime.render()
    })?
}

pub fn runtime_snapshot() -> GuiResult<GuiRuntimeSnapshot> {
    with_runtime(|runtime| runtime.snapshot())
}

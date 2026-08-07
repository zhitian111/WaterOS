//! WaterOS GUI 的纯软件实现。
//!
//! 所有图元先绘制到 shadow surface；提交阶段才短暂锁定显示设备，把脏区域复制到
//! framebuffer。窗口、事件和渲染均不依赖具体 VirtIO transport。

#![no_std]
extern crate alloc;

mod canvas;
mod demo;
mod font;
mod global;
mod input;
mod runtime;
mod scene;
mod surface;
mod theme;

pub use canvas::Canvas;
pub use demo::{
    ACTION_BUTTON, COMMAND_INPUT, MAIN_WINDOW, PROGRESS, STATUS_LABEL, SYSTEM_LABEL,
    SYSTEM_WINDOW, install_default_desktop, update_default_desktop,
};
pub use global::{
    add_window, initialize, initialize_on, is_initialized, mark_dirty, poll_event,
    poll_hardware_input, process_pending_input, push_input, remove_window, render,
    render_if_dirty, runtime_snapshot, set_label_text, set_progress, set_theme, shutdown,
    with_runtime,
};
pub use input::InputBridge;
pub use runtime::{GuiRuntime, GuiRuntimeSnapshot};
pub use surface::{DirtyRegions, ShadowSurface};
pub use theme::Theme;

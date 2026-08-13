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

#[cfg(feature = "self_test")]
pub fn self_test() {
    let mut surface = ShadowSurface::new(api_v0::Size::new(8, 8))
        .expect("GUI self_test surface allocation");
    assert_eq!(surface.pixels().len(), 8 * 8 * surface::BYTES_PER_PIXEL);
    surface.pixels_mut()[0] = 0xaa;
    assert_eq!(surface.pixels()[0], 0xaa);

    let mut dirty = DirtyRegions::new(api_v0::Rect::new(0, 0, 8, 8));
    dirty.add(api_v0::Rect::new(-2, -2, 4, 4));
    assert_eq!(dirty.take().len(), 1);
    assert!(dirty.is_empty());
}

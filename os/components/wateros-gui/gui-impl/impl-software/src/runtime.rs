//! GUI 实例：事件队列、桌面状态、shadow surface 和显示提交。

use alloc::collections::VecDeque;
use api_v0::{
    GuiError, GuiEvent, GuiResult, InputEvent, Rect, Size, WidgetId, Window, WindowId,
};
use display::{FramebufferRegion, PixelFormat, SharedDisplayDevice};

use crate::{Canvas, DirtyRegions, InputBridge, ShadowSurface, Theme, scene::Desktop};

const INPUT_QUEUE_CAPACITY : usize = 256;
const OUTPUT_QUEUE_CAPACITY : usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuiRuntimeSnapshot {
    pub size : Size,
    pub windows : usize,
    pub pending_input : usize,
    pub pending_output : usize,
    pub frames_rendered : u64,
    pub frames_presented : u64,
    pub dropped_input : u64,
    pub input_events_received : u64,
    pub dirty : bool,
}

/// 单显示器 GUI 实例。锁顺序固定为 GUI runtime → display device。
pub struct GuiRuntime {
    display : SharedDisplayDevice,
    surface : ShadowSurface,
    desktop : Desktop,
    theme : Theme,
    dirty : DirtyRegions,
    input : VecDeque<InputEvent>,
    input_bridge : InputBridge,
    output : VecDeque<GuiEvent>,
    frames_rendered : u64,
    frames_presented : u64,
    dropped_input : u64,
    input_events_received : u64,
}

impl GuiRuntime {
    pub fn new(display : SharedDisplayDevice) -> GuiResult<Self> {
        let info = display.lock().info();
        if info.width == 0 || info.height == 0 || info.stride < info.width as usize * 4 ||
           info.format != PixelFormat::Bgra8888
        {
            return Err(GuiError::InvalidSurface);
        }
        let size = Size::new(info.width, info.height);
        let surface = ShadowSurface::new(size)?;
        let mut dirty = DirtyRegions::new(Rect::from_size(size));
        dirty.mark_all();
        Ok(Self { display,
                  surface,
                  desktop : Desktop::new(),
                  theme : Theme::default(),
                  dirty,
                  input : VecDeque::with_capacity(INPUT_QUEUE_CAPACITY),
                  input_bridge : InputBridge::new(),
                  output : VecDeque::with_capacity(OUTPUT_QUEUE_CAPACITY),
                  frames_rendered : 0,
                  frames_presented : 0,
                  dropped_input : 0,
                  input_events_received : 0 })
    }

    pub fn size(&self) -> Size { self.surface.size() }

    pub const fn theme(&self) -> &Theme { &self.theme }

    pub fn set_theme(&mut self, theme : Theme) {
        self.theme = theme;
        self.dirty.mark_all();
    }

    /// 标记调用方直接修改所影响的区域；区域会自动裁剪到屏幕范围。
    pub fn mark_dirty(&mut self, region : Rect) { self.dirty.add(region); }

    pub fn snapshot(&self) -> GuiRuntimeSnapshot {
        GuiRuntimeSnapshot { size : self.size(),
                             windows : self.desktop.windows().len(),
                             pending_input : self.input.len(),
                             pending_output : self.output.len(),
                             frames_rendered : self.frames_rendered,
                             frames_presented : self.frames_presented,
                             dropped_input : self.dropped_input,
                             input_events_received : self.input_events_received,
                             dirty : !self.dirty.is_empty() }
    }

    pub fn add_window(&mut self, window : Window) {
        self.desktop.add_window(window);
        self.dirty.mark_all();
    }

    pub fn remove_window(&mut self, id : WindowId) -> bool {
        let changed = self.desktop.remove_window(id);
        if changed {
            self.dirty.mark_all();
        }
        changed
    }

    pub fn set_label_text(&mut self,
                          window : WindowId,
                          widget : WidgetId,
                          text : alloc::string::String)
                          -> GuiResult<()> {
        match self.desktop.set_label_text(window, widget, text) {
            Ok(Some(rect)) => self.dirty.add(rect),
            Ok(None) => {}
            Err(()) => return Err(GuiError::WidgetNotFound),
        }
        Ok(())
    }

    pub fn set_progress(&mut self, window : WindowId, widget : WidgetId, value : u32)
                        -> GuiResult<()> {
        match self.desktop.set_progress(window, widget, value) {
            Ok(Some(rect)) => self.dirty.add(rect),
            Ok(None) => {}
            Err(()) => return Err(GuiError::WidgetNotFound),
        }
        Ok(())
    }

    pub fn push_input(&mut self, event : InputEvent) -> GuiResult<()> {
        if self.input.len() >= INPUT_QUEUE_CAPACITY {
            self.dropped_input = self.dropped_input.saturating_add(1);
            return Err(GuiError::QueueFull);
        }
        self.input.push_back(event);
        Ok(())
    }

    pub fn poll_event(&mut self) -> Option<GuiEvent> { self.output.pop_front() }

    /// 从所有已注册硬件输入设备中提取一批事件并放入 GUI 队列。
    /// 每帧设有上限，避免事件风暴长期占用 GUI 全局锁。
    pub fn poll_hardware_input(&mut self) -> usize {
        let events = self.input_bridge.poll(self.size(), 128);
        let count = events.len();
        self.input_events_received = self.input_events_received.saturating_add(count as u64);
        for event in events {
            let _ = self.push_input(event);
        }
        count
    }

    pub fn process_pending_input(&mut self) -> usize {
        let mut processed = 0;
        while let Some(input) = self.input.pop_front() {
            if self.desktop.handle_input(input, &mut self.output) {
                // 窗口拖动和 pointer 光标可能跨多个区域，先保证正确性；控件 API
                // 的定向更新仍会保留精细脏区。
                self.dirty.mark_all();
            }
            processed += 1;
            while self.output.len() > OUTPUT_QUEUE_CAPACITY {
                self.output.pop_front();
            }
        }
        processed
    }

    pub fn render(&mut self) -> GuiResult<bool> {
        if self.dirty.is_empty() {
            return Ok(false);
        }
        let regions = self.dirty.take();
        {
            let mut canvas = Canvas::new(&mut self.surface);
            self.desktop.render(&mut canvas, &self.theme);
        }
        self.frames_rendered = self.frames_rendered.saturating_add(1);
        if let Err(error) = self.present_regions(&regions) {
            for region in regions {
                self.dirty.add(region);
            }
            return Err(error);
        }
        self.frames_presented = self.frames_presented.saturating_add(1);
        Ok(true)
    }

    fn present_regions(&mut self, regions : &[Rect]) -> GuiResult<()> {
        let mut display = self.display.lock();
        let info = display.info();
        let framebuffer = display.framebuffer().map_err(|_| GuiError::DisplayFailure)?;
        for region in regions {
            copy_region(&self.surface, framebuffer, info.stride, *region)?;
        }
        let Some(bounds) = regions.iter().copied().reduce(Rect::union) else { return Ok(()) };
        display.flush_region(FramebufferRegion {
            x : bounds.origin.x.max(0) as u32,
            y : bounds.origin.y.max(0) as u32,
            width : bounds.size.width,
            height : bounds.size.height,
        }).map_err(|_| GuiError::DisplayFailure)
    }
}

fn copy_region(surface : &ShadowSurface,
               framebuffer : &mut [u8],
               framebuffer_stride : usize,
               region : Rect)
               -> GuiResult<()> {
    let Some(region) = region.intersection(surface.bounds()) else { return Ok(()) };
    let row_bytes = region.size.width as usize * 4;
    for row in 0..region.size.height as usize {
        let source_start = (region.origin.y as usize + row) * surface.stride() +
                           region.origin.x as usize * 4;
        let target_start = (region.origin.y as usize + row) * framebuffer_stride +
                           region.origin.x as usize * 4;
        let source = surface.pixels().get(source_start..source_start + row_bytes)
                            .ok_or(GuiError::InvalidSurface)?;
        let target = framebuffer.get_mut(target_start..target_start + row_bytes)
                                .ok_or(GuiError::InvalidSurface)?;
        target.copy_from_slice(source);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_capacity_is_bounded() {
        // 真实 runtime 需要 DisplayDevice；此处只锁定容量常量的非零契约。
        assert!(INPUT_QUEUE_CAPACITY >= 64);
        assert!(OUTPUT_QUEUE_CAPACITY >= 64);
    }
}

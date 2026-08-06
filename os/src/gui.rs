//! 可选的内核软件绘制演示。
//!
//! 本模块不拥有 GPU，也不创建长期刷新任务。它在显示驱动注册完成后借用第一个
//! framebuffer，绘制一次欢迎页并主动刷新，用于验证 QEMU → VirtIO → DMA →
//! framebuffer 的完整通路。

use driver::display::{DriverError, DriverResult, FramebufferInfo, PixelFormat};

/// Canvas 使用的 RGB 颜色；写入 framebuffer 时转换为 BGRA8888。
#[derive(Clone, Copy)]
struct Color {
    red : u8,
    green : u8,
    blue : u8,
}

impl Color {
    const fn rgb(red : u8, green : u8, blue : u8) -> Self { Self { red, green, blue } }
}

/// 对显示设备线性 framebuffer 的短期可写视图。
struct Canvas<'a> {
    info : FramebufferInfo,
    pixels : &'a mut [u8],
}

impl Canvas<'_> {
    fn clear(&mut self, color : Color) {
        self.fill_rect(0,
                       0,
                       self.info.width,
                       self.info.height,
                       color);
    }

    fn fill_rect(&mut self, x : u32, y : u32, width : u32, height : u32, color : Color) {
        let right = x.saturating_add(width)
                     .min(self.info.width);
        let bottom = y.saturating_add(height)
                      .min(self.info.height);
        for py in y.min(self.info.height)..bottom {
            for px in x.min(self.info.width)..right {
                self.put_pixel(px, py, color);
            }
        }
    }

    fn put_pixel(&mut self, x : u32, y : u32, color : Color) {
        if x >= self.info.width || y >= self.info.height {
            return;
        }
        let offset = y as usize * self.info.stride + x as usize * 4;
        let Some(pixel) = self.pixels
                              .get_mut(offset..offset + 4)
        else {
            return;
        };
        match self.info.format {
            PixelFormat::Bgra8888 => {
                pixel[0] = color.blue;
                pixel[1] = color.green;
                pixel[2] = color.red;
                pixel[3] = 0xFF;
            }
        }
    }

    /// 使用内嵌 5x7 位图绘制大写 ASCII。未收录字符显示为空格。
    fn draw_text(&mut self, mut x : u32, y : u32, text : &str, scale : u32, color : Color) {
        let scale = scale.max(1);
        for character in text.bytes() {
            self.draw_glyph(x, y, glyph(character), scale, color);
            x = x.saturating_add(6 * scale);
        }
    }

    fn draw_glyph(&mut self, x : u32, y : u32, rows : [u8; 7], scale : u32, color : Color) {
        for (row, bits) in rows.into_iter()
                               .enumerate()
        {
            for column in 0..5u32 {
                if bits & (1 << (4 - column)) == 0 {
                    continue;
                }
                self.fill_rect(x + column * scale,
                               y + row as u32 * scale,
                               scale,
                               scale,
                               color);
            }
        }
    }
}

/// 在第一个显示设备上绘制 WaterOS 欢迎页。没有 GPU 时返回 `NotFound`。
pub fn draw_boot_screen() -> DriverResult<()> {
    let device = driver::display::first_display_device().ok_or(DriverError::NotFound)?;
    let mut device = device.lock();
    let info = device.info();
    let pixels = device.framebuffer()?;
    {
        let mut canvas = Canvas { info, pixels };
        canvas.clear(Color::rgb(9, 18, 38));

        let margin = (info.width / 16).max(16);
        let header_height = (info.height / 5).max(84)
                                             .min(info.height);
        canvas.fill_rect(0,
                         0,
                         info.width,
                         header_height,
                         Color::rgb(22, 72, 145));
        canvas.fill_rect(margin,
                         header_height.saturating_add(margin),
                         info.width
                             .saturating_sub(margin * 2),
                         info.height
                             .saturating_sub(header_height + margin * 2),
                         Color::rgb(15, 31, 59));

        let title_scale = if info.width >= 800 { 6 } else { 4 };
        canvas.draw_text(margin,
                         header_height / 2 - 4 * title_scale,
                         "WATEROS",
                         title_scale,
                         Color::rgb(240, 247, 255));
        let body_scale = if info.width >= 640 { 3 } else { 2 };
        let body_y = header_height.saturating_add(margin * 2);
        canvas.draw_text(margin * 2,
                         body_y,
                         "VIRTIO GPU READY",
                         body_scale,
                         Color::rgb(83, 214, 170));
        canvas.draw_text(margin * 2,
                         body_y.saturating_add(12 * body_scale),
                         "KERNEL FRAMEBUFFER",
                         body_scale,
                         Color::rgb(167, 190, 224));

        // 右下角状态灯可在没有完整字体/格式化支持时直观看出 flush 是否成功。
        let lamp = (info.height / 24).clamp(12, 32);
        canvas.fill_rect(info.width
                             .saturating_sub(margin + lamp),
                         info.height
                             .saturating_sub(margin + lamp),
                         lamp,
                         lamp,
                         Color::rgb(83, 214, 170));
    }
    device.flush()
}

/// 5x7 字模，每行低 5 位从左到右表示像素。
fn glyph(character : u8) -> [u8; 7] {
    match character.to_ascii_uppercase() {
        b'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        b'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        b'D' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E],
        b'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        b'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        b'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0E],
        b'I' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x1F],
        b'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        b'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        b'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        b'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        b'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        b'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        b'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        b'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        b'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        b'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        b'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        b'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        b'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        b' ' => [0; 7],
        _ => [0; 7],
    }
}

//! 带裁剪的 BGRA8888 软件 Canvas。

use alloc::vec::Vec;
use api_v0::{Color, Point, Rect, TextMetrics, TextStyle};

use crate::{font, surface::BYTES_PER_PIXEL, ShadowSurface};

pub struct Canvas<'a> {
    pixels : &'a mut [u8],
    stride : usize,
    bounds : Rect,
    clip : Rect,
}

impl<'a> Canvas<'a> {
    pub fn new(surface : &'a mut ShadowSurface) -> Self {
        let bounds = surface.bounds();
        let stride = surface.stride();
        Self { pixels : surface.pixels_mut(),
               stride,
               bounds,
               clip : bounds }
    }

    pub const fn bounds(&self) -> Rect { self.bounds }

    pub const fn clip(&self) -> Rect { self.clip }

    /// 设置裁剪区并返回旧值；空交集会令后续绘制全部跳过。
    pub fn set_clip(&mut self, clip : Rect) -> Rect {
        let old = self.clip;
        self.clip = self.bounds.intersection(clip).unwrap_or(Rect::EMPTY);
        old
    }

    pub fn restore_clip(&mut self, clip : Rect) { self.clip = clip; }

    pub fn clear(&mut self, color : Color) { self.fill_rect(self.bounds, color); }

    pub fn put_pixel(&mut self, point : Point, color : Color) {
        if !self.clip.contains(point) {
            return;
        }
        let offset = point.y as usize * self.stride + point.x as usize * BYTES_PER_PIXEL;
        let Some(pixel) = self.pixels.get_mut(offset..offset + BYTES_PER_PIXEL) else {
            return;
        };
        let color = if color.alpha == 255 {
            color
        } else {
            let background = Color::from_bgra8888([pixel[0], pixel[1], pixel[2], pixel[3]]);
            color.blend_over(background)
        };
        let mut encoded = color.to_bgra8888();
        encoded[3] = 255;
        pixel.copy_from_slice(&encoded);
    }

    pub fn fill_rect(&mut self, rect : Rect, color : Color) {
        let Some(rect) = rect.intersection(self.clip) else {
            return;
        };
        if color.alpha == 255 {
            for y in rect.origin.y..rect.bottom() {
                let start = y as usize * self.stride + rect.origin.x as usize * BYTES_PER_PIXEL;
                let end = start + rect.size.width as usize * BYTES_PER_PIXEL;
                if let Some(row) = self.pixels.get_mut(start..end) {
                    for pixel in row.chunks_exact_mut(BYTES_PER_PIXEL) {
                        let mut encoded = color.to_bgra8888();
                        encoded[3] = 255;
                        pixel.copy_from_slice(&encoded);
                    }
                }
            }
        } else {
            for y in rect.origin.y..rect.bottom() {
                for x in rect.origin.x..rect.right() {
                    self.put_pixel(Point::new(x, y), color);
                }
            }
        }
    }

    pub fn stroke_rect(&mut self, rect : Rect, thickness : u32, color : Color) {
        let thickness = thickness.min(rect.size.width / 2).min(rect.size.height / 2);
        if thickness == 0 {
            return;
        }
        self.fill_rect(Rect::new(rect.origin.x, rect.origin.y, rect.size.width, thickness), color);
        self.fill_rect(Rect::new(rect.origin.x,
                                 rect.bottom().saturating_sub(thickness as i32),
                                 rect.size.width,
                                 thickness),
                       color);
        self.fill_rect(Rect::new(rect.origin.x, rect.origin.y, thickness, rect.size.height), color);
        self.fill_rect(Rect::new(rect.right().saturating_sub(thickness as i32),
                                 rect.origin.y,
                                 thickness,
                                 rect.size.height),
                       color);
    }

    /// Bresenham 整数直线。
    pub fn draw_line(&mut self, start : Point, end : Point, color : Color) {
        let (mut x, mut y) = (start.x, start.y);
        let dx = (end.x - start.x).abs();
        let sx = if start.x < end.x { 1 } else { -1 };
        let dy = -(end.y - start.y).abs();
        let sy = if start.y < end.y { 1 } else { -1 };
        let mut error = dx + dy;
        loop {
            self.put_pixel(Point::new(x, y), color);
            if x == end.x && y == end.y {
                break;
            }
            let twice = error.saturating_mul(2);
            if twice >= dy {
                error += dy;
                x += sx;
            }
            if twice <= dx {
                error += dx;
                y += sy;
            }
        }
    }

    pub fn draw_circle(&mut self, center : Point, radius : u32, color : Color) {
        let mut x = radius as i32;
        let mut y = 0i32;
        let mut error = 1 - x;
        while x >= y {
            for (dx, dy) in [(x, y), (y, x), (-y, x), (-x, y),
                             (-x, -y), (-y, -x), (y, -x), (x, -y)] {
                self.put_pixel(Point::new(center.x + dx, center.y + dy), color);
            }
            y += 1;
            if error < 0 {
                error += 2 * y + 1;
            } else {
                x -= 1;
                error += 2 * (y - x) + 1;
            }
        }
    }

    pub fn fill_circle(&mut self, center : Point, radius : u32, color : Color) {
        let radius = radius as i32;
        for y in -radius..=radius {
            let span = integer_sqrt((radius * radius - y * y) as u32) as i32;
            self.draw_line(Point::new(center.x - span, center.y + y),
                           Point::new(center.x + span, center.y + y),
                           color);
        }
    }

    /// 依次连接所有顶点；少于两个点时不绘制。
    pub fn draw_polyline(&mut self, points : &[Point], color : Color) {
        for edge in points.windows(2) {
            self.draw_line(edge[0], edge[1], color);
        }
    }

    /// 绘制闭合多边形轮廓。
    pub fn draw_polygon(&mut self, points : &[Point], color : Color) {
        self.draw_polyline(points, color);
        if points.len() >= 3 {
            self.draw_line(points[points.len() - 1], points[0], color);
        }
    }

    /// 使用奇偶规则扫描线填充多边形。凹多边形可用，自相交图形按奇偶规则处理。
    pub fn fill_polygon(&mut self, points : &[Point], color : Color) {
        if points.len() < 3 || self.clip == Rect::EMPTY {
            return;
        }
        let minimum_y = points.iter().map(|point| point.y).min().unwrap_or(0)
                              .max(self.clip.origin.y);
        let maximum_y = points.iter().map(|point| point.y).max().unwrap_or(-1)
                              .min(self.clip.bottom() - 1);
        let mut intersections = Vec::with_capacity(points.len());
        for y in minimum_y..=maximum_y {
            intersections.clear();
            for index in 0..points.len() {
                let first = points[index];
                let second = points[(index + 1) % points.len()];
                if (first.y <= y && second.y > y) || (second.y <= y && first.y > y) {
                    let numerator = i64::from(y - first.y) * i64::from(second.x - first.x);
                    let denominator = i64::from(second.y - first.y);
                    intersections.push(first.x.saturating_add((numerator / denominator) as i32));
                }
            }
            intersections.sort_unstable();
            for pair in intersections.chunks_exact(2) {
                self.draw_line(Point::new(pair[0], y), Point::new(pair[1], y), color);
            }
        }
    }

    /// 从外部 BGRA8888 缓冲复制一个矩形，源和目标均会裁剪。
    pub fn blit_bgra(&mut self,
                     source : &[u8],
                     source_stride : usize,
                     source_rect : Rect,
                     destination : Point) {
        if source_rect.origin.x < 0 || source_rect.origin.y < 0 {
            return;
        }
        let destination_rect = Rect::new(destination.x,
                                         destination.y,
                                         source_rect.size.width,
                                         source_rect.size.height);
        let Some(clipped) = destination_rect.intersection(self.clip) else {
            return;
        };
        let source_x = source_rect.origin.x as usize +
                       (clipped.origin.x - destination.x) as usize;
        let source_y = source_rect.origin.y as usize +
                       (clipped.origin.y - destination.y) as usize;
        let row_bytes = clipped.size.width as usize * BYTES_PER_PIXEL;
        for row in 0..clipped.size.height as usize {
            let src_start = (source_y + row) * source_stride + source_x * BYTES_PER_PIXEL;
            let dst_start = (clipped.origin.y as usize + row) * self.stride +
                            clipped.origin.x as usize * BYTES_PER_PIXEL;
            let Some(src) = source.get(src_start..src_start + row_bytes) else { break };
            let Some(dst) = self.pixels.get_mut(dst_start..dst_start + row_bytes) else { break };
            dst.copy_from_slice(src);
        }
    }

    pub fn measure_text(&self, text : &str, bounds : Rect, style : TextStyle) -> TextMetrics {
        font::measure_text(text, bounds, style)
    }

    pub fn draw_text(&mut self, bounds : Rect, text : &str, style : TextStyle) -> TextMetrics {
        font::draw_text(self, bounds, text, style)
    }
}

fn integer_sqrt(value : u32) -> u32 {
    if value < 2 {
        return value;
    }
    let mut x = value;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + value / x) / 2;
    }
    x
}

#[cfg(test)]
mod tests {
    use super::*;
    use api_v0::Size;

    #[test]
    fn clipping_prevents_writes_outside_region() {
        let mut surface = ShadowSurface::new(Size::new(8, 8)).unwrap();
        let mut canvas = Canvas::new(&mut surface);
        canvas.clear(Color::BLACK);
        canvas.set_clip(Rect::new(2, 2, 2, 2));
        canvas.fill_rect(Rect::new(0, 0, 8, 8), Color::WHITE);
        drop(canvas);
        let lit = surface.pixels().chunks_exact(4).filter(|pixel| pixel[0] == 255).count();
        assert_eq!(lit, 4);
    }

    #[test]
    fn polygon_fill_covers_interior_without_leaving_surface() {
        let mut surface = ShadowSurface::new(Size::new(16, 16)).unwrap();
        let mut canvas = Canvas::new(&mut surface);
        canvas.clear(Color::BLACK);
        canvas.fill_polygon(&[Point::new(2, 2), Point::new(13, 2), Point::new(8, 13)],
                            Color::WHITE);
        drop(canvas);
        let pixel = |x : usize, y : usize| surface.pixels()[(y * 16 + x) * 4];
        assert_eq!(pixel(8, 6), 255);
        assert_eq!(pixel(0, 0), 0);
    }
}

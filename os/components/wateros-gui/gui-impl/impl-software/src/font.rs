//! 内嵌 5×7 可打印 ASCII 字体与文本布局。

use alloc::{string::String, vec::Vec};
use api_v0::{Point, Rect, TextAlign, TextMetrics, TextStyle, TextWrap, VerticalAlign};

use crate::Canvas;

pub const GLYPH_WIDTH : u32 = 5;
/// 字形位图高度（像素），不含行间距。
pub const GLYPH_HEIGHT : u32 = 7;
/// 相邻字形的水平步进（像素），含一列间隔。
pub const GLYPH_ADVANCE : u32 = 6;
/// 相邻文本行的垂直步进（像素），含一行间隔。
pub const LINE_ADVANCE : u32 = 8;

pub fn measure_text(text : &str, bounds : Rect, style : TextStyle) -> TextMetrics {
    // scale 至少为 1，避免零缩放导致除零或返回看似可见但无法绘制的文本。
    let scale = u32::from(style.scale.max(1));
    let lines = layout_lines(text, bounds.size.width, scale, style.wrap);
    let width = lines.iter()
                     .map(|line| line.chars().count() as u32 * GLYPH_ADVANCE * scale)
                     .max()
                     .unwrap_or(0)
                     .min(bounds.size.width);
    TextMetrics { width,
                  height : (lines.len() as u32 * LINE_ADVANCE * scale).min(bounds.size.height),
                  lines : lines.len() as u32 }
}

pub fn draw_text(canvas : &mut Canvas<'_>,
                 bounds : Rect,
                 text : &str,
                 style : TextStyle)
                 -> TextMetrics {
    let Some(bounds) = bounds.intersection(canvas.clip()) else {
        return TextMetrics::default();
    };
    if let Some(background) = style.background {
        canvas.fill_rect(bounds, background);
    }
    let scale = u32::from(style.scale.max(1));
    let lines = layout_lines(text, bounds.size.width, scale, style.wrap);
    let total_height = lines.len() as u32 * LINE_ADVANCE * scale;
    let mut y = match style.vertical {
        VerticalAlign::Top => bounds.origin.y,
        VerticalAlign::Middle => bounds.origin.y +
                                 bounds.size.height.saturating_sub(total_height) as i32 / 2,
        VerticalAlign::Bottom => bounds.bottom() - total_height.min(bounds.size.height) as i32,
    };
    let mut maximum_width = 0;
    let old_clip = canvas.set_clip(bounds);
    for line in &lines {
        if y >= bounds.bottom() {
            break;
        }
        let line_width = line.chars().count() as u32 * GLYPH_ADVANCE * scale;
        maximum_width = maximum_width.max(line_width);
        let mut x = match style.horizontal {
            TextAlign::Left => bounds.origin.x,
            TextAlign::Center => bounds.origin.x +
                                 bounds.size.width.saturating_sub(line_width) as i32 / 2,
            TextAlign::Right => bounds.right() - line_width.min(bounds.size.width) as i32,
        };
        for character in line.chars() {
            draw_glyph(canvas,
                       Point::new(x, y),
                       glyph(character),
                       scale,
                       style.foreground);
            x = x.saturating_add((GLYPH_ADVANCE * scale) as i32);
        }
        y = y.saturating_add((LINE_ADVANCE * scale) as i32);
    }
    canvas.restore_clip(old_clip);
    TextMetrics { width : maximum_width.min(bounds.size.width),
                  height : total_height.min(bounds.size.height),
                  lines : lines.len() as u32 }
}

fn draw_glyph(canvas : &mut Canvas<'_>,
              origin : Point,
              rows : [u8; 7],
              scale : u32,
              color : api_v0::Color) {
    debug_assert_eq!(rows.len(), GLYPH_HEIGHT as usize);
    for (row, bits) in rows.into_iter().enumerate() {
        for column in 0..GLYPH_WIDTH {
            if bits & (1 << (GLYPH_WIDTH - 1 - column)) != 0 {
                canvas.fill_rect(Rect::new(origin.x + (column * scale) as i32,
                                           origin.y + (row as u32 * scale) as i32,
                                           scale,
                                           scale),
                                 color);
            }
        }
    }
}

fn layout_lines(text : &str, width : u32, scale : u32, wrap : TextWrap) -> Vec<String> {
    // 极窄区域仍保留每行一个字符的容量，保证换行结果和绘制过程终止。
    let capacity = (width / (GLYPH_ADVANCE * scale)).max(1) as usize;
    let mut output = Vec::new();
    for explicit in text.split('\n') {
        match wrap {
            TextWrap::NoWrap => output.push(explicit.chars().take(capacity).collect()),
            TextWrap::Character => push_character_wrapped(&mut output, explicit, capacity),
            TextWrap::Word => push_word_wrapped(&mut output, explicit, capacity),
        }
    }
    if output.is_empty() {
        output.push(String::new());
    }
    output
}

fn push_character_wrapped(output : &mut Vec<String>, text : &str, capacity : usize) {
    if text.is_empty() {
        output.push(String::new());
        return;
    }
    let chars : Vec<char> = text.chars().collect();
    for chunk in chars.chunks(capacity) {
        output.push(chunk.iter().collect());
    }
}

fn push_word_wrapped(output : &mut Vec<String>, text : &str, capacity : usize) {
    if text.trim().is_empty() {
        output.push(String::new());
        return;
    }
    let mut current = String::new();
    for word in text.split_whitespace() {
        let word_len = word.chars().count();
        if word_len > capacity {
            if !current.is_empty() {
                output.push(core::mem::take(&mut current));
            }
            push_character_wrapped(output, word, capacity);
            continue;
        }
        let required = current.chars().count() + usize::from(!current.is_empty()) + word_len;
        if required > capacity {
            output.push(core::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        output.push(current);
    }
}

/// 完整覆盖 ASCII 0x20..0x7e；非 ASCII 字符使用问号占位。
fn glyph(character : char) -> [u8; 7] {
    match character {
        ' ' => [0, 0, 0, 0, 0, 0, 0],
        '!' => [0x04, 0x04, 0x04, 0x04, 0x04, 0x00, 0x04],
        '"' => [0x0a, 0x0a, 0x0a, 0, 0, 0, 0],
        '#' => [0x0a, 0x1f, 0x0a, 0x0a, 0x1f, 0x0a, 0],
        '$' => [0x04, 0x0f, 0x14, 0x0e, 0x05, 0x1e, 0x04],
        '%' => [0x18, 0x19, 0x02, 0x04, 0x08, 0x13, 0x03],
        '&' => [0x0c, 0x12, 0x14, 0x08, 0x15, 0x12, 0x0d],
        '\'' => [0x04, 0x04, 0x08, 0, 0, 0, 0],
        '(' => [0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02],
        ')' => [0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08],
        '*' => [0, 0x15, 0x0e, 0x1f, 0x0e, 0x15, 0],
        '+' => [0, 0x04, 0x04, 0x1f, 0x04, 0x04, 0],
        ',' => [0, 0, 0, 0, 0x06, 0x04, 0x08],
        '-' => [0, 0, 0, 0x1f, 0, 0, 0],
        '.' => [0, 0, 0, 0, 0, 0x0c, 0x0c],
        '/' => [0x01, 0x02, 0x02, 0x04, 0x08, 0x08, 0x10],
        '0' => [0x0e, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0e],
        '1' => [0x04, 0x0c, 0x14, 0x04, 0x04, 0x04, 0x1f],
        '2' => [0x0e, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1f],
        '3' => [0x1e, 0x01, 0x01, 0x0e, 0x01, 0x01, 0x1e],
        '4' => [0x02, 0x06, 0x0a, 0x12, 0x1f, 0x02, 0x02],
        '5' => [0x1f, 0x10, 0x10, 0x1e, 0x01, 0x01, 0x1e],
        '6' => [0x0e, 0x10, 0x10, 0x1e, 0x11, 0x11, 0x0e],
        '7' => [0x1f, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0e, 0x11, 0x11, 0x0e, 0x11, 0x11, 0x0e],
        '9' => [0x0e, 0x11, 0x11, 0x0f, 0x01, 0x01, 0x0e],
        ':' => [0, 0x0c, 0x0c, 0, 0x0c, 0x0c, 0],
        ';' => [0, 0x0c, 0x0c, 0, 0x0c, 0x08, 0x10],
        '<' => [0x02, 0x04, 0x08, 0x10, 0x08, 0x04, 0x02],
        '=' => [0, 0, 0x1f, 0, 0x1f, 0, 0],
        '>' => [0x08, 0x04, 0x02, 0x01, 0x02, 0x04, 0x08],
        '?' => [0x0e, 0x11, 0x01, 0x02, 0x04, 0, 0x04],
        '@' => [0x0e, 0x11, 0x17, 0x15, 0x17, 0x10, 0x0e],
        'A' => [0x0e, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11],
        'B' => [0x1e, 0x11, 0x11, 0x1e, 0x11, 0x11, 0x1e],
        'C' => [0x0e, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0e],
        'D' => [0x1e, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1e],
        'E' => [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x1f],
        'F' => [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x10],
        'G' => [0x0e, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0e],
        'H' => [0x11, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11],
        'I' => [0x0e, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0e],
        'J' => [0x07, 0x02, 0x02, 0x02, 0x12, 0x12, 0x0c],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1f],
        'M' => [0x11, 0x1b, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        'O' => [0x0e, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e],
        'P' => [0x1e, 0x11, 0x11, 0x1e, 0x10, 0x10, 0x10],
        'Q' => [0x0e, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0d],
        'R' => [0x1e, 0x11, 0x11, 0x1e, 0x14, 0x12, 0x11],
        'S' => [0x0f, 0x10, 0x10, 0x0e, 0x01, 0x01, 0x1e],
        'T' => [0x1f, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0a, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0a],
        'X' => [0x11, 0x11, 0x0a, 0x04, 0x0a, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0a, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1f, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1f],
        '[' => [0x0e, 0x08, 0x08, 0x08, 0x08, 0x08, 0x0e],
        '\\' => [0x10, 0x08, 0x08, 0x04, 0x02, 0x02, 0x01],
        ']' => [0x0e, 0x02, 0x02, 0x02, 0x02, 0x02, 0x0e],
        '^' => [0x04, 0x0a, 0x11, 0, 0, 0, 0],
        '_' => [0, 0, 0, 0, 0, 0, 0x1f],
        '`' => [0x08, 0x04, 0x02, 0, 0, 0, 0],
        'a' => [0, 0, 0x0e, 0x01, 0x0f, 0x11, 0x0f],
        'b' => [0x10, 0x10, 0x16, 0x19, 0x11, 0x11, 0x1e],
        'c' => [0, 0, 0x0e, 0x10, 0x10, 0x11, 0x0e],
        'd' => [0x01, 0x01, 0x0d, 0x13, 0x11, 0x11, 0x0f],
        'e' => [0, 0, 0x0e, 0x11, 0x1f, 0x10, 0x0e],
        'f' => [0x06, 0x09, 0x08, 0x1c, 0x08, 0x08, 0x08],
        'g' => [0, 0x0f, 0x11, 0x11, 0x0f, 0x01, 0x0e],
        'h' => [0x10, 0x10, 0x16, 0x19, 0x11, 0x11, 0x11],
        'i' => [0x04, 0, 0x0c, 0x04, 0x04, 0x04, 0x0e],
        'j' => [0x02, 0, 0x06, 0x02, 0x02, 0x12, 0x0c],
        'k' => [0x10, 0x10, 0x12, 0x14, 0x18, 0x14, 0x12],
        'l' => [0x0c, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0e],
        'm' => [0, 0, 0x1a, 0x15, 0x15, 0x15, 0x15],
        'n' => [0, 0, 0x16, 0x19, 0x11, 0x11, 0x11],
        'o' => [0, 0, 0x0e, 0x11, 0x11, 0x11, 0x0e],
        'p' => [0, 0, 0x1e, 0x11, 0x1e, 0x10, 0x10],
        'q' => [0, 0, 0x0f, 0x11, 0x0f, 0x01, 0x01],
        'r' => [0, 0, 0x16, 0x19, 0x10, 0x10, 0x10],
        's' => [0, 0, 0x0f, 0x10, 0x0e, 0x01, 0x1e],
        't' => [0x08, 0x08, 0x1c, 0x08, 0x08, 0x09, 0x06],
        'u' => [0, 0, 0x11, 0x11, 0x11, 0x13, 0x0d],
        'v' => [0, 0, 0x11, 0x11, 0x11, 0x0a, 0x04],
        'w' => [0, 0, 0x11, 0x11, 0x15, 0x15, 0x0a],
        'x' => [0, 0, 0x11, 0x0a, 0x04, 0x0a, 0x11],
        'y' => [0, 0, 0x11, 0x11, 0x0f, 0x01, 0x0e],
        'z' => [0, 0, 0x1f, 0x02, 0x04, 0x08, 0x1f],
        '{' => [0x02, 0x04, 0x04, 0x08, 0x04, 0x04, 0x02],
        '|' => [0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        '}' => [0x08, 0x04, 0x04, 0x02, 0x04, 0x04, 0x08],
        '~' => [0, 0, 0x09, 0x16, 0, 0, 0],
        _ => [0x0e, 0x11, 0x01, 0x02, 0x04, 0, 0x04],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use api_v0::{Color, Size};
    use crate::ShadowSurface;

    #[test]
    fn word_wrapping_and_ascii_rendering_work() {
        let lines = layout_lines("hello WaterOS", 7 * GLYPH_ADVANCE, 1, TextWrap::Word);
        assert_eq!(lines, ["hello", "WaterOS"]);
        let mut surface = ShadowSurface::new(Size::new(80, 16)).unwrap();
        let mut canvas = Canvas::new(&mut surface);
        let metrics = draw_text(&mut canvas,
                                Rect::new(0, 0, 80, 16),
                                "aZ09!?",
                                TextStyle { foreground : Color::WHITE,
                                            ..TextStyle::default() });
        assert_eq!(metrics.lines, 1);
        assert!(surface.pixels().iter().any(|value| *value != 0));
    }
}

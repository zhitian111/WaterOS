//! 文本测量、换行和对齐参数。

use crate::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerticalAlign {
    Top,
    Middle,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextWrap {
    NoWrap,
    Character,
    Word,
}

/// 位图字体的绘制样式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextStyle {
    pub foreground : Color,
    pub background : Option<Color>,
    pub scale : u8,
    pub horizontal : TextAlign,
    pub vertical : VerticalAlign,
    pub wrap : TextWrap,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self { foreground : Color::WHITE,
               background : None,
               scale : 1,
               horizontal : TextAlign::Left,
               vertical : VerticalAlign::Top,
               wrap : TextWrap::NoWrap }
    }
}

/// 文本布局后的像素尺寸和行数。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TextMetrics {
    pub width : u32,
    pub height : u32,
    pub lines : u32,
}

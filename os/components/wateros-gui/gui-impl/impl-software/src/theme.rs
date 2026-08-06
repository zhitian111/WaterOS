//! 控件渲染使用的集中式主题。

use api_v0::Color;

#[derive(Debug, Clone, Copy)]
/// 软件 renderer 使用的完整颜色主题。替换主题会触发全屏重绘。
pub struct Theme {
    pub desktop : Color,
    pub desktop_accent : Color,
    pub window : Color,
    pub window_border : Color,
    pub title_active : Color,
    pub title_inactive : Color,
    pub text : Color,
    pub text_muted : Color,
    pub control : Color,
    pub control_hover : Color,
    pub control_pressed : Color,
    pub focus : Color,
    pub progress_track : Color,
    pub progress_fill : Color,
    pub danger : Color,
}

impl Default for Theme {
    fn default() -> Self {
        Self { desktop : Color::rgb(7, 15, 31),
               desktop_accent : Color::rgb(13, 31, 58),
               window : Color::rgb(18, 29, 47),
               window_border : Color::rgb(67, 91, 126),
               title_active : Color::rgb(27, 91, 174),
               title_inactive : Color::rgb(49, 62, 82),
               text : Color::rgb(238, 245, 255),
               text_muted : Color::rgb(157, 177, 205),
               control : Color::rgb(43, 64, 92),
               control_hover : Color::rgb(55, 86, 124),
               control_pressed : Color::rgb(25, 51, 82),
               focus : Color::rgb(74, 222, 178),
               progress_track : Color::rgb(31, 47, 69),
               progress_fill : Color::rgb(53, 199, 151),
               danger : Color::rgb(225, 77, 87) }
    }
}

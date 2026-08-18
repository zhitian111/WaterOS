//! 控件渲染使用的集中式主题。

use api_v0::Color;

#[derive(Debug, Clone, Copy)]
/// 软件 renderer 使用的完整颜色主题。替换主题会触发全屏重绘。
pub struct Theme {
    /// 桌面底色。
    pub desktop : Color,
    /// 桌面装饰区域颜色。
    pub desktop_accent : Color,
    /// 普通窗口背景色。
    pub window : Color,
    /// 窗口边框颜色。
    pub window_border : Color,
    /// 活动窗口标题栏颜色。
    pub title_active : Color,
    /// 非活动窗口标题栏颜色。
    pub title_inactive : Color,
    /// 主文本颜色。
    pub text : Color,
    /// 禁用或占位文本颜色。
    pub text_muted : Color,
    /// 普通控件背景色。
    pub control : Color,
    /// 鼠标悬停控件背景色。
    pub control_hover : Color,
    /// 按下控件背景色。
    pub control_pressed : Color,
    /// 焦点框和插入符颜色。
    pub focus : Color,
    /// 进度条未完成轨道颜色。
    pub progress_track : Color,
    /// 进度条已完成部分颜色。
    pub progress_fill : Color,
    /// 关闭按钮等危险操作颜色。
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

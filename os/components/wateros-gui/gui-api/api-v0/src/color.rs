//! 颜色模型与整数 alpha 混合。

/// 与硬件像素格式无关的 RGBA 颜色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    /// 红色通道，范围 0..=255。
    pub red : u8,
    /// 绿色通道，范围 0..=255。
    pub green : u8,
    /// 蓝色通道，范围 0..=255。
    pub blue : u8,
    /// 非预乘 alpha，0 为完全透明、255 为完全不透明。
    pub alpha : u8,
}

impl Color {
    pub const TRANSPARENT : Self = Self::rgba(0, 0, 0, 0);
    pub const BLACK : Self = Self::rgb(0, 0, 0);
    pub const WHITE : Self = Self::rgb(255, 255, 255);

   pub const fn rgb(red : u8, green : u8, blue : u8) -> Self {
        // RGB 构造器约定不透明，透明颜色须显式使用 `rgba`。
        Self::rgba(red, green, blue, 255)
    }

    pub const fn rgba(red : u8, green : u8, blue : u8, alpha : u8) -> Self {
        Self { red,
               green,
               blue,
               alpha }
    }

    /// 编码成当前显示后端使用的 BGRA8888 内存顺序。
    pub const fn to_bgra8888(self) -> [u8; 4] {
        [self.blue, self.green, self.red, self.alpha]
    }

    /// 从 BGRA8888 内存顺序解码。
    pub const fn from_bgra8888(pixel : [u8; 4]) -> Self {
        Self::rgba(pixel[2], pixel[1], pixel[0], pixel[3])
    }

   /// 使用整数运算把当前前景色覆盖到不透明背景色上。
    /// 返回值始终是不透明颜色；半透明背景的 alpha 不参与计算。
    pub fn blend_over(self, background : Self) -> Self {
        let alpha = u32::from(self.alpha);
        let inverse = 255 - alpha;
        let channel = |foreground : u8, background : u8| {
            ((u32::from(foreground) * alpha + u32::from(background) * inverse + 127) / 255) as u8
        };
        Self::rgb(channel(self.red, background.red),
                  channel(self.green, background.green),
                  channel(self.blue, background.blue))
    }
}

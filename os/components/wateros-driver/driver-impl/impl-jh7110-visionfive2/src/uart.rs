//! JH7110 UART 字符设备接线：从 DTB `reg-shift`/`reg-io-width` 推导寄存器布局。

use api_v0::MmioRegion;
use character::RegisterLayout;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UartDescription {
    pub mmio : MmioRegion,
    pub layout : RegisterLayout,
}

pub fn register(uart : UartDescription) -> usize {
    character::register_uart_character_device(uart.mmio.base, uart.layout)
}

pub(crate) fn layout(reg_shift : Option<u32>,
                     reg_io_width : Option<u32>)
                     -> Option<RegisterLayout> {
    match (reg_shift.unwrap_or(0), reg_io_width.unwrap_or(1)) {
        (0, 1) => Some(RegisterLayout::Byte16550),
        (2, 4) => Some(RegisterLayout::DwApb32),
        _ => None,
    }
}

/// 纯函数自检：验证已知寄存器布局映射。
pub fn test() {
    assert_eq!(layout(None, None),
               Some(RegisterLayout::Byte16550));
    assert_eq!(layout(Some(0), Some(1)),
               Some(RegisterLayout::Byte16550));
    assert_eq!(layout(Some(2), Some(4)),
               Some(RegisterLayout::DwApb32));
    assert_eq!(layout(Some(2), Some(1)), None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_known_register_layout_pairs() {
        assert_eq!(layout(None, None),
                   Some(RegisterLayout::Byte16550));
        assert_eq!(layout(Some(0), Some(1)),
                   Some(RegisterLayout::Byte16550));
        assert_eq!(layout(Some(2), Some(4)),
                   Some(RegisterLayout::DwApb32));
        assert_eq!(layout(Some(2), Some(1)), None);
        assert_eq!(layout(Some(1), Some(4)), None);
        assert_eq!(layout(Some(4), Some(8)), None);
    }
}

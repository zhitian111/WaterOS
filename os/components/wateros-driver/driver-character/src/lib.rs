//! 字符设备子系统入口：串口 API（v0）与 DTB 声明表。

#![no_std]

use driver_api::SupportedDeviceEntry;

/// 字符设备 API v0（[`SerialPort`](api_v0::SerialPort) 等）。
pub mod api_v0 {
    pub use character_api_v0::*;
}

/// 占位函数，保持与子 crate 骨架一致；非字符设备 API。
///
/// **当前行为**：无 I/O；**后续替换点**：接入更多子系统后可删除。
pub fn add(left : u64, right : u64) -> u64 {
    left + right
}

/// 字符设备子系统当前未声明任何 DTB 绑定条目；占位供聚合层遍历。
pub fn supported_devices() -> &'static [SupportedDeviceEntry] {
    &[]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}

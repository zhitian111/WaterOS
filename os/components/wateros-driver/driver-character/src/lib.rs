//! 字符设备子系统入口：当前仅提供 DTB 声明表（空）与占位符号，具体 tty 等尚未接入。

#![no_std]

use driver_api::SupportedDeviceEntry;

/// 占位函数，保持与子 crate 骨架一致；非字符设备 API。
pub fn add(left: u64, right: u64) -> u64 {
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

//! 网络设备子系统入口：当前无 DTB 声明条目，占位供聚合扫描与后续 NIC 驱动接入。

#![no_std]

use driver_api::SupportedDeviceEntry;

/// 占位函数，保持与子 crate 骨架一致。
///
/// **当前行为**：无 NIC 绑定；**后续替换点**：与 `network-api` 对齐后移除此占位。
pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

/// 网络子系统当前未声明任何 DTB 绑定条目；占位供聚合层遍历。
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

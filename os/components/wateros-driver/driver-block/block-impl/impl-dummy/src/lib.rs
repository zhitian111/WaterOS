//! 块子系统占位实现：无硬件绑定，保留最小 crate 边界以便 feature 组合。
//!
//! 后续若需「无块设备」语义，应在此集中说明而非散落在调用方。

#![no_std]

/// 占位算术函数（crate 骨架）；与驱动无关，仅供依赖解析与单测通过。
pub fn add(left : u64, right : u64) -> u64 { left + right }
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}

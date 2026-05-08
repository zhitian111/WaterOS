//! 网络设备占位实现。

#![no_std]

/// 占位算术函数；非驱动逻辑。
///
/// **当前行为**：无硬件、无协议栈挂钩；**后续替换点**：virtio-net 等实现 crate。
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

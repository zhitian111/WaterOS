//! 平台驱动占位实现：不解析 DTB、不注册设备，用于无硬件目标的构建与依赖占位。

#![no_std]

/// 占位算术函数。
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

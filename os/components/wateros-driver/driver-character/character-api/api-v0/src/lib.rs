//! 字符设备 API（v0）占位 crate：类型与 I/O trait 将在接入控制台/tty 时填充。

#![no_std]

/// 占位算术函数；非正式 API。
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

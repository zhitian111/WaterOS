#![no_std]
//! 信号 IPC 聚合占位：子 crate 未挂入主 workspace。

/// 占位函数。
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

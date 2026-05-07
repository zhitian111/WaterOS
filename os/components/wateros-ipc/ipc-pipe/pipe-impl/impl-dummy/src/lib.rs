#![no_std]
//! 管道 dummy 实现占位：链接期占位，行为未定义。

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

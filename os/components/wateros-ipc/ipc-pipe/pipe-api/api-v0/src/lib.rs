#![no_std]
//! 管道 API v0 占位：后续在此定义句柄、缓冲区与非阻塞标志等契约。

/// 占位函数，非正式 API。
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

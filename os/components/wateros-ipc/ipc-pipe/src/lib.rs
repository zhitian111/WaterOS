#![no_std]
//! 管道 IPC 聚合占位：子 crate（`pipe-api` / `pipe-impl`）尚未挂入主 workspace。

/// 占位函数，仅用于本地测试；无 pipe 语义。
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

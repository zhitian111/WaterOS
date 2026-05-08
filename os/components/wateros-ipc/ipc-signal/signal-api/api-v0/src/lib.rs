#![no_std]
//! 信号 API v0 占位。
//!
//! 后续在此固定信号集表示、掩码/挂起队列抽象、以及与用户态 ABI 对齐的 `repr`；dummy 实现不应假装已覆盖异步信号安全边界。

/// 占位算术：无 `kill`/掩码/待决位等语义。
pub fn add(left : u64, right : u64) -> u64 { left + right }

// 信号 API v0 占位 crate 的编译期自检。
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}

#![no_std]
//! 信号 dummy 实现占位。
//!
//! 与 `signal-api` 对齐前的链接桩：不投递、不排队、不操纵线程信号掩码；接入真实调度/陷阱路径后在此实现 API 并注明与上下文保存的交互假设。

/// 占位算术：无信号投递语义。
pub fn add(left : u64, right : u64) -> u64 { left + right }

// dummy 实现 crate 的链接自检。
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}

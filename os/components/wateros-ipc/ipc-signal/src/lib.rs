#![no_std]
//! 信号 IPC 聚合占位：子 crate 未挂入主 workspace。
//!
//! `signal-api` / `signal-impl` 将承载投递、掩码与与陷阱上下文的交互约定；本聚合层在接入前应只作边界占位。

/// 占位算术：无信号编号、栈帧或异步安全语义；真实实现需与 `ipc-signal` 子 crate 对齐后替换。
pub fn add(left : u64, right : u64) -> u64 { left + right }

// 占位 crate 的编译期自检。
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}

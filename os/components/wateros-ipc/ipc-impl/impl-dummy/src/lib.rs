#![no_std]
//! IPC 聚合所选 dummy 实现：当前为占位符号，与真实 IPC 路径无关。
//!
//! 被 `wateros-ipc` 在 `feature = "impl-dummy"` 下作为 `active_impl` 重导出；替换为真实 impl 时需保持 crate 名或聚合 `pub use` 路径稳定，以免调用方大面积改动。

/// 占位算术：满足链接与单测；**不**模拟任何 syscall 或资源生命周期。
pub fn add(left : u64, right : u64) -> u64 { left + right }

// 保证 dummy impl 在单测中可执行，便于 CI 覆盖 feature 组合。
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}

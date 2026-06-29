#![no_std]
//! IPC API v0 占位门面：保留 crate 边界与测试钩子，真实系统调用号与句柄类型将在此演进。
//!
//! 与 `wateros-ipc` 聚合 crate 的关系：聚合层的 `api` 模块重导出本包；对外契约（错误码、资源句柄、`repr(C)` 布局）应在此 crate 固化后再向下游传播。

/// 占位算术函数，仅用于依赖图与单元测试；**不是**正式 IPC 语义的一部分。
pub fn add(left : u64, right : u64) -> u64 { left + right }

// API 门面 crate 的基础链接自检。
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}

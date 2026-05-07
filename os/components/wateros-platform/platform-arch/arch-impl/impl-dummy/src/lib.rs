#![no_std]

//! **占位**架构实现：用于未启用真实 ISA profile 的构建或依赖占位；非生产路径。
//!
//! 与 `impl-riscv64` 互斥由上层 feature 选择；当前保留最小可编译表面，后续可替换为
//! 与 `arch-api` 对齐的 no-op 类型集。

/// 占位算术（仅用于保留测试/链接样例，与平台无关）。
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

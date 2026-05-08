#![no_std]
//! ABI 占位实现 crate：用于工作区依赖解析与编译连通性，不包含真实平台语义。
//!
//! 对外符号仅为 Cargo 模板级占位，后续可由具体 `impl-*` 包替换或删除。
//!
//! English: stub `impl-*` crate for workspace resolution and `cargo check` graphs;
//! replace with a real platform table when wiring features.

/// 模板级占位函数，仅供本 crate 内建单测；与内核或用户态 ABI 无关。
///
/// English: trivial helper for the default crate test only; not part of WaterOS ABI.
pub fn add(left : u64, right : u64) -> u64 { left + right }

#[cfg(test)]
mod tests {
    use super::*;

    // 验证占位算术路径可执行。 / Smoke-test the stub arithmetic path.
    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}

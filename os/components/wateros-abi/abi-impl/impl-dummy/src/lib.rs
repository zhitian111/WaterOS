#![no_std]
//! ABI 占位实现 crate：用于工作区依赖解析与编译连通性，不包含真实平台语义。
//!
//! 对外符号仅为 Cargo 模板级占位，后续可由具体 `impl-*` 包替换或删除。

/// 模板级占位函数，仅供本 crate 内建单测；与内核或用户态 ABI 无关。
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

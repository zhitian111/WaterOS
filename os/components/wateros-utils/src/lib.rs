#![no_std]
//! 通用小工具与可复用例程的聚合 crate（当前为早期占位）。
//!
//! 后续可在此集中与内核其它组件无强耦合的纯函数或数据结构；避免反向依赖 `wateros-base` 等平台类型以保持 DAG 清晰。

/// 模板级占位函数，仅供本 crate 内建单测；不代表最终公共 API。
pub fn add(left : u64, right : u64) -> u64 { left + right }

#[cfg(test)]
mod tests {
    use super::*;

    // 占位 API 的烟测：验证 crate 可独立 `cargo test` 链接。
    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}

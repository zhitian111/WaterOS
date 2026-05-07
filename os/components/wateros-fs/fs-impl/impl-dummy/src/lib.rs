#![no_std]

//! 占位实现 crate：随 `wateros-fs` 工作区构建保留，不作为运行时 FS 路径依赖。
//!
//! 边界：其中 `add` 仅为 Cargo 模板级示例 API；若将来删除需同步清理依赖图中未引用告警。

/// 无语义占位函数（模板遗留）；勿在内核路径依赖其行为。
pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}

#![no_std]
//! 管道 API v0 占位：后续在此定义句柄、缓冲区与非阻塞标志等契约。
//!
//! 与 `ipc-pipe` 聚合及 `pipe-impl` 的边界：用户可见类型与错误应在此定义；实现 crate 只依赖本 API 而不反向暴露内核细节。

/// 占位算术：无管道 I/O 契约；句柄类型与缓冲语义落地后应移除或改名以避免与正式 API 混淆。
pub fn add(left : u64, right : u64) -> u64 { left + right }

// 管道 API v0 占位 crate 的编译期自检。
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}

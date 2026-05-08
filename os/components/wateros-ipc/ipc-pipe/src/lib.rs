#![no_std]
//! 管道 IPC 聚合占位：子 crate（`pipe-api` / `pipe-impl`）尚未挂入主 workspace。
//!
//! 与 `ipc-pipe/pipe-api`、`pipe-impl` 的职责划分：本 crate 将来可作为管道子树的聚合门面；现阶段仅保证目录与 Cargo 边界存在。

/// 占位算术：无 read/write/EOF 等管道语义；正式管道 API 在 `pipe-api` 演进后应由本 crate 或上层聚合重导出。
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

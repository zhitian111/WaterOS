#![no_std]
//! 共享内存 IPC 子模块占位：crate 与符号边界预留，尚未接入 `wateros-ipc` 聚合依赖。

/// 占位函数，仅用于本地测试与空实现编译；无 SHM 语义。
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

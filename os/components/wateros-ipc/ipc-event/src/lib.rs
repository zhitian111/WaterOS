#![no_std]
//! 事件/同步原语 IPC 子模块占位。

/// 占位函数，非正式 API。
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

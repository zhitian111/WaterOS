#![no_std]
//! IPC 聚合所选 dummy 实现：当前为占位符号，与真实 IPC 路径无关。

/// 占位函数，供依赖解析与单测。
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

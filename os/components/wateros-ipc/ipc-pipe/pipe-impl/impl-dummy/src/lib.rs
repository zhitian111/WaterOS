#![no_std]
//! 管道 dummy 实现占位：链接期占位，行为未定义。
//!
//! 当前行为：不读写内核缓冲、不注册 fd 表项；仅提供符号以便 `pipe-api` 侧依赖图闭合。替换为真实 impl 时需实现 `pipe-api` 所声明的操作并处理错误传播。

/// 占位算术：无管道实现语义。
pub fn add(left : u64, right : u64) -> u64 { left + right }

// dummy 实现 crate 的链接自检。
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}

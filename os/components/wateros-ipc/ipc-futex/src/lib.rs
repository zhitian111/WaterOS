#![no_std]
//! Futex 风格等待子模块占位。
//!
//! 用途：为基于 futex 的睡眠/唤醒路径预留 crate 与 feature 接线点。当前无用户可见地址字、队列或调度交互；与 `ipc-waitqueue` 的关系待架构收敛后写明。

/// 占位算术：仅供构建与单测；**无** futex 等待语义。真实实现需与 MMU、任务阻塞原语对齐后再提供对外 API。
pub fn add(left : u64, right : u64) -> u64 { left + right }

// 占位符号可被测试 harness 引用。
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}

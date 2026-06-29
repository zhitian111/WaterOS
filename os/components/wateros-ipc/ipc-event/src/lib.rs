#![no_std]
//! 事件/同步原语 IPC 子模块占位。
//!
//! 用途：预留「事件对象 / 同步」类 IPC 的 crate 边界与构建钩子。当前无内核事件队列或句柄语义；后续替换时应在此定义类型与 `wateros-ipc` 聚合的依赖关系。

/// 占位算术：仅用于依赖解析与单测编译，**不是**事件 IPC 契约的一部分；正式 API 落地后应删除或替换为真实入口。
#[inline]
pub fn add(left : u64, right : u64) -> u64 { left + right }

// 验证占位符号可被链接与测试 harness 引用。
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}

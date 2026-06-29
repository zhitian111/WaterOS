//! 本模块代码由AI完成
//! 字符设备占位实现：无硬件、无 DTB 绑定。

#![no_std]

/// 占位算术函数；非驱动逻辑。
///
/// **当前行为**：无 DTB、无设备注册；**后续替换点**：真实字符设备实现 crate。
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

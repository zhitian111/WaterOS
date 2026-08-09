//! 本模块代码由AI完成
//! 平台驱动占位实现：不解析 DTB、不注册设备，用于无硬件目标的构建与依赖占位。

#![no_std]

use api_v0::{DriverResult, MachineDriver};

/// 当前 dummy profile 的机器驱动单例。
pub struct Machine;

static MACHINE: Machine = Machine;

/// 返回当前机器的 [`MachineDriver`] 契约实现。
pub fn machine() -> &'static dyn MachineDriver {
    &MACHINE
}

impl MachineDriver for Machine {
    fn init_when_boot(&self, _dtb_pa: usize) {}

    fn init_after_boot(&self) -> DriverResult<()> {
        Ok(())
    }

    fn test(&self) {
        log::info!("[driver] dummy impl: skip qemu probe test");
    }
}

/// 占位算术函数。
///
/// **当前行为**：不解析 DTB、不调用 `init_after_boot` 语义；**后续替换点**：由 feature 选中的平台 impl 替代。
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

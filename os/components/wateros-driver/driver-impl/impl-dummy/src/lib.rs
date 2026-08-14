//! 无硬件占位 driver profile。
//!
//! 用于没有真实设备的构建目标：不解析 DTB、不注册设备，仅提供 [`MachineDriver`]
//! 的最小实现，保证聚合层 `machine()` 有可用单例。外部中断与 per-CPU 中断初始化
//! 使用 trait 默认语义（`Unsupported` / `Ok(())`）。

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
    fn init_after_boot(&self) -> DriverResult<()> {
        log::info!("[driver] dummy profile: no devices to register");
        Ok(())
    }

    fn test(&self) {
        log::info!("[driver] dummy profile: skip hardware probe test");
    }
}

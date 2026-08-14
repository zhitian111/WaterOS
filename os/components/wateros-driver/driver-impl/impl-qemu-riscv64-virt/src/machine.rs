//! 当前 profile 的 [`MachineDriver`] 契约实现。

use api_v0::{DriverError, DriverResult, MachineDriver};

use crate::{enumerate, init_after_boot, test};

/// 当前 QEMU RISC-V profile 的机器驱动单例。
pub struct Machine;

static MACHINE: Machine = Machine;

/// 返回当前机器的 [`MachineDriver`] 契约实现。
pub fn machine() -> &'static dyn MachineDriver {
    &MACHINE
}

impl MachineDriver for Machine {
    fn init_after_boot(&self) -> DriverResult<()> {
        init_after_boot()
    }

    fn realtime_ns(&self) -> DriverResult<Option<u64>> {
        enumerate::goldfish_rtc_realtime_ns().map(Some)
    }

    fn handle_external_interrupt(&self, _cpu_raw : usize) -> DriverResult<bool> {
        // QEMU `virt` 当前不使能 S 态外部中断（`sie.SEIE` 未打开，PLIC 也未纳入
        // 恒等映射），本 profile 不路由外部中断；板级 PLIC 派发由 JH7110 驱动
        // profile（任务 06）实现。
        Err(DriverError::Unsupported)
    }

    fn test(&self) {
        test::test()
    }
}

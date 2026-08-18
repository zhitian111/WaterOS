//! 当前 profile 的 [`MachineDriver`] 契约实现。

use api_v0::{DriverError, DriverResult, MachineDriver};

use crate::{enumerate, init_after_boot, test};

/// 当前 QEMU LoongArch64 profile 的机器驱动单例。
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
        enumerate::ls7a_rtc_realtime_ns().map(Some)
    }

    fn handle_external_interrupt(&self, _cpu_raw : usize) -> DriverResult<bool> {
        // QEMU LoongArch64 `virt` 当前只使用 CSR 定时器中断；EXTIOI/LS7A 外部中断
        // 未配置，本 profile 不路由外部中断。2K1000 板级 LIOINTC 派发在任务 11。
        Err(DriverError::Unsupported)
    }

    fn test(&self) {
        test::test()
    }
}

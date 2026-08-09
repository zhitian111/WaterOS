//! 当前 profile 的 [`MachineDriver`] 契约实现。

use api_v0::{DriverResult, MachineDriver};

use crate::{boot, init_after_boot, test};

/// 当前 QEMU LoongArch64 profile 的机器驱动单例。
pub struct Machine;

static MACHINE: Machine = Machine;

/// 返回当前机器的 [`MachineDriver`] 契约实现。
pub fn machine() -> &'static dyn MachineDriver {
    &MACHINE
}

impl MachineDriver for Machine {
    fn init_when_boot(&self, dtb_pa: usize) {
        boot::init_when_boot(dtb_pa)
    }

    fn init_after_boot(&self) -> DriverResult<()> {
        init_after_boot()
    }

    fn test(&self) {
        test::test()
    }
}

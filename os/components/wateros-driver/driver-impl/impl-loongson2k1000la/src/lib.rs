//! Loongson 2K1000LA 机器驱动：PCIe ECAM 上探测 AHCI/SATA 并注册块设备。
//!
//! 第一阶段为 polled PIO（无外部中断）；LIOINTC 等外部中断接入在任务 11。

#![no_std]

use api_v0::{DriverResult, MachineDriver};

pub struct Machine;
static MACHINE : Machine = Machine;

pub fn machine() -> &'static dyn MachineDriver {
    &MACHINE
}

impl MachineDriver for Machine {
    fn init_after_boot(&self) -> DriverResult<()> {
        match ahci::init() {
            Ok(index) => {
                log::info!("[driver][2k1000] AHCI/SATA registered as block device #{}",
                           index);
                Ok(())
            }
            Err(error) => {
                log::warn!("[driver][2k1000] AHCI probe failed: {:?}",
                           error);
                Err(error)
            }
        }
    }

    fn test(&self) {
        log::info!("[driver][2k1000] machine test: AHCI is polled-PIO, no MMIO touched");
    }
}

#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[driver][2k1000] self_test begin");
    Machine.test();
    log::info!("[driver][2k1000] self_test complete");
}

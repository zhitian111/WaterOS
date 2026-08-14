//! Loongson 2K1000LA 机器驱动：PCIe ECAM 上探测 AHCI/SATA 并注册块设备。
//!
//! 第一阶段为 polled PIO（无外部中断）；LIOINTC 等外部中断接入在任务 11。

#![no_std]

use api_v0::{DriverResult, MachineDriver};

pub mod liointc;

pub struct Machine;
static MACHINE : Machine = Machine;

pub fn machine() -> &'static dyn MachineDriver {
    &MACHINE
}

impl MachineDriver for Machine {
    fn init_after_boot(&self) -> DriverResult<()> {
        #[cfg(target_arch = "loongarch64")]
        {
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
        #[cfg(not(target_arch = "loongarch64"))]
        {
            Ok(())
        }
    }

    fn handle_external_interrupt(&self, cpu_raw : usize) -> DriverResult<bool> {
        #[cfg(target_arch = "loongarch64")]
        {
            liointc::handle_external_interrupt_la()
        }
        #[cfg(not(target_arch = "loongarch64"))]
        {
            let _ = cpu_raw;
            Err(api_v0::DriverError::Unsupported)
        }
    }

    fn init_current_cpu(&self, cpu_raw : usize) -> DriverResult<()> {
        liointc::init_current_cpu(cpu_raw)
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

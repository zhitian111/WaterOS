//! Loongson 2K1000LA 机器驱动：DTB UART、RTC、LIOINTC 与 AHCI/SATA。
//!
//! 第一阶段为 polled PIO（无外部中断）；LIOINTC 等外部中断接入在任务 11。

#![no_std]

use api_v0::{DriverResult, MachineDriver};

pub mod liointc;
pub mod rtc;
pub mod uart;

pub struct Machine;
static MACHINE : Machine = Machine;

pub fn machine() -> &'static dyn MachineDriver { &MACHINE }

#[cfg(target_arch = "loongarch64")]
fn platform_dtb_pa() -> usize { platform::dtb_pa() }

#[cfg(not(target_arch = "loongarch64"))]
fn platform_dtb_pa() -> usize { 0 }

impl MachineDriver for Machine {
    fn init_after_boot(&self) -> DriverResult<()> {
        #[cfg(target_arch = "loongarch64")]
        {
            if let Err(error) = uart::register_from_dtb(platform_dtb_pa()) {
                log::warn!("[driver][2k1000] UART probe failed: {:?}",
                           error);
            }
            character::register_builtin_character_devices();
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

    fn realtime_ns(&self) -> DriverResult<Option<u64>> {
        rtc::realtime_ns(platform_dtb_pa()).map(Some)
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
        uart::test();
        rtc::test();
        liointc::test();
        log::info!("[driver][2k1000] machine test: UART/RTC/LIOINTC/AHCI hooks ready");
    }
}

#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[driver][2k1000] self_test begin");
    Machine.test();
    log::info!("[driver][2k1000] self_test complete");
}

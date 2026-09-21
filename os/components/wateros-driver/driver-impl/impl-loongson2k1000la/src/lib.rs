//! Loongson 2K1000LA 机器驱动：DTB UART、RTC、GMAC 与 AHCI/SATA。

#![no_std]
extern crate alloc;

use api_v0::{DriverResult, MachineDriver};

pub mod gmac;
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
            if let Err(error) = gmac::register_from_dtb(platform_dtb_pa()) {
                log::warn!("[driver][2k1000] GMAC probe failed: {:?}",
                           error);
            }
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

    fn test(&self) {
        uart::test();
        rtc::test();
        gmac::test();
        log::info!("[driver][2k1000] machine test: UART/RTC/AHCI/GMAC hooks ready");
    }
}

#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[driver][2k1000] self_test begin");
    Machine.test();
    log::info!("[driver][2k1000] self_test complete");
}

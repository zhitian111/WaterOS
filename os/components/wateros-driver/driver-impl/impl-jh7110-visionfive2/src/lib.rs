//! VisionFive 2 / JH7110 机器驱动：DTB 优先的设备发现与延迟硬件激活（fail-closed）。

#![no_std]
extern crate alloc;

pub mod irq;
pub mod mmc;
pub mod plic;
pub mod topology;
pub mod uart;

use api_v0::{DriverResult, MachineDriver};
use core::sync::atomic::{AtomicBool, Ordering};

static INITIALIZED : AtomicBool = AtomicBool::new(false);
pub struct Machine;
static MACHINE : Machine = Machine;

pub fn machine() -> &'static dyn MachineDriver {
    &MACHINE
}

#[cfg(target_arch = "riscv64")]
fn platform_dtb_pa() -> usize {
    platform::dtb_pa()
}
#[cfg(not(target_arch = "riscv64"))]
fn platform_dtb_pa() -> usize {
    0
}

impl MachineDriver for Machine {
    fn init_after_boot(&self) -> DriverResult<()> {
        if INITIALIZED.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        let result = topology::discover(platform_dtb_pa()).and_then(|board| {
                         topology::store(board.clone());
                         character::register_builtin_character_devices();
                         if let Some(console) = board.console_uart {
                             let index = uart::register(console);
                             log::info!("[driver][visionfive2] registered console uart #{} \
                                         base={:#x} layout={:?}",
                                        index,
                                        console.mmio.base,
                                        console.layout);
                         } else {
                             log::warn!("[driver][visionfive2] DTB has no supported /chosen \
                                         console UART; early console remains available");
                         }
                         if let Some(plic) = board.plic {
                             log::info!("[driver][visionfive2] PLIC discovered base={:#x} \
                                         size={:#x} sources={} contexts={}; source activation \
                                         requires an explicit IRQ handler",
                                        plic.mmio.base,
                                        plic.mmio.size,
                                        plic.sources,
                                        plic.contexts.len());
                         }
                         for host in &board.mmc_hosts {
                             let plan = mmc::bring_up_plan(host);
                             log::info!("[driver][visionfive2] MMC bring-up plan base={:#x} \
                                         irq={} width={} max_hz={:?} blockers={:?}; \
                                         activation=UNVERIFIED",
                                        plan.host.mmio.base,
                                        plan.host.irq,
                                        plan.host.bus_width,
                                        plan.host.max_frequency_hz,
                                        plan.blockers);
                         }
                         Ok(())
                     });
        if result.is_err() {
            INITIALIZED.store(false, Ordering::Release);
        }
        result
    }

    fn handle_external_interrupt(&self, cpu_raw : usize) -> DriverResult<bool> {
        irq::handle_external_interrupt(cpu_raw)
    }

    fn init_current_cpu(&self, cpu_raw : usize) -> DriverResult<()> {
        irq::initialize_current_hart(cpu_raw)
    }

    fn test(&self) {
        topology::with_topology(|topology| {
            log::info!("[driver][visionfive2] topology={:?}; hardware MMIO/IRQ status=UNVERIFIED",
                       topology)
        });
    }
}

#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[driver][visionfive2] self_test begin");
    test::self_test_body();
    log::info!("[driver][visionfive2] self_test complete");
}

/// 只读自检：复用 [`MachineDriver::test`] 的拓扑日志路径。
#[cfg(feature = "self_test")]
mod test {
    pub fn self_test_body() {
        super::Machine.test();
    }
}

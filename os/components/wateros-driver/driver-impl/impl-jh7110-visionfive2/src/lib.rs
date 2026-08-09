//! VisionFive 2 machine driver: DTB-first discovery with deferred hardware activation.
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

pub fn machine() -> &'static dyn MachineDriver { &MACHINE }

#[cfg(target_arch = "riscv64")]
fn platform_dtb_pa() -> usize { platform::dtb_pa() }
#[cfg(not(target_arch = "riscv64"))]
fn platform_dtb_pa() -> usize { 0 }

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
                             log::info!("[driver][visionfive2] MMC host discovered base={:#x} \
                                         irq={} width={} max_hz={:?}; clock/reset/card bring-up \
                                         status=UNVERIFIED",
                                        host.mmio.base,
                                        host.irq,
                                        host.bus_width,
                                        host.max_frequency_hz);
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

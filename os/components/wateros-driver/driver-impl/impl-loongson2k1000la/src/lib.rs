//! Loongson 2K1000LA DTB-first machine driver.
//!
//! This profile currently discovers and validates board resources only. It
//! deliberately performs no LIOINTC, MMC clock/reset or DMA register writes:
//! those activation paths are `UNVERIFIED_ON_HARDWARE` until a board is
//! available.

#![no_std]
extern crate alloc;

pub mod irq_domain;
pub mod liointc;
pub mod mmc;
pub mod apbdma;
pub mod dma_memory;
mod machine;
pub mod topology;

use api_v0::{DriverError, DriverResult};
use spin::Mutex;
use topology::BoardTopology;

static TOPOLOGY : Mutex<Option<BoardTopology>> = Mutex::new(None);

pub use machine::machine;

#[cfg(target_arch = "loongarch64")]
pub fn init_after_boot() -> DriverResult<()> {
    let fdt = common::dtb::read_fdt(platform::dtb_pa())?;
    let topology = topology::discover(&fdt)?;
    log::info!("[driver-ls2k] topology: uart={} liointc={} dma={} mmc={}",
               topology.uarts.len(),
               topology.interrupt_controllers
                       .len(),
               topology.dma_controllers
                       .len(),
               topology.mmc_hosts
                       .len());
    for host in &topology.mmc_hosts {
        let plan = mmc::plan(host).map_err(|error| {
                                      log::error!("[driver-ls2k][mmc] invalid deferred plan: {:?}",
                                                  error);
                                      DriverError::InvalidDtb
                                  })?;
        log::warn!("[driver-ls2k][mmc] deferred controller={:#x}/{:#x} \
                    auxiliary={:#x}/{:#x} bus_width={} blockers={:?}; \
                    hardware activation=UNVERIFIED_ON_HARDWARE",
                   plan.controller_mmio.base,
                   plan.controller_mmio.size,
                   plan.auxiliary_mmio.base,
                   plan.auxiliary_mmio.size,
                   plan.bus_width,
                   plan.blockers);
    }
    *TOPOLOGY.lock() = Some(topology);
    log::warn!("[driver-ls2k] resources discovered but hardware activation is \
                UNVERIFIED_ON_HARDWARE");
    Ok(())
}

#[cfg(not(target_arch = "loongarch64"))]
pub fn init_after_boot() -> DriverResult<()> { Err(DriverError::Unsupported) }

pub fn with_topology<R>(f : impl FnOnce(Option<&BoardTopology>) -> R) -> R {
    let topology = TOPOLOGY.lock();
    f(topology.as_ref())
}

fn self_test() {
    if with_topology(|topology| topology.is_none()) {
        log::info!("[driver-ls2k] topology not initialized; skip runtime self-test");
    }
}

fn unsupported_realtime() -> DriverResult<Option<u64>> { Err(DriverError::Unsupported) }

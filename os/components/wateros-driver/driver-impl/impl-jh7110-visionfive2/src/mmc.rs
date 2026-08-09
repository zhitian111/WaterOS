//! VisionFive 2 MMC resources and compatibility exports.
//!
//! Clock/reset/syscon descriptions belong to this board layer. Controller PIO
//! and SD protocol logic live in `wateros-driver-block-impl-dw-mmc` so another
//! platform can reuse them without importing JH7110 topology assumptions.
use alloc::vec::Vec;
use api_v0::MmioRegion;

pub use dw_mmc::mmc::{clock_divider, DwMmc, MmcError, MmioRegisters, RegisterIo};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceSpecifier {
    pub provider : u32,
    pub args : Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SysregField {
    pub provider : u32,
    pub offset : u32,
    pub shift : u8,
    pub mask : u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MmcHostDescription {
    pub mmio : MmioRegion,
    pub irq : u32,
    pub bus_width : u8,
    pub max_frequency_hz : Option<u32>,
    pub fifo_depth : Option<u32>,
    pub non_removable : bool,
    pub biu_clock : ResourceSpecifier,
    pub ciu_clock : ResourceSpecifier,
    pub reset : ResourceSpecifier,
    pub sysreg : Option<SysregField>,
}

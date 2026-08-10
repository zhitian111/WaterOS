//! VisionFive 2 MMC resources and compatibility exports.
//!
//! Clock/reset/syscon descriptions belong to this board layer. Controller PIO
//! and SD protocol logic live in `wateros-driver-block-impl-dw-mmc` so another
//! platform can reuse them without importing JH7110 topology assumptions.
use alloc::vec::Vec;
use api_v0::MmioRegion;

pub use dw_mmc::mmc::{clock_divider, DwMmc, MmcError, MmioRegisters, RegisterIo};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmcActivationBlocker {
    InvalidMmio,
    InvalidIrq,
    InvalidBusWidth,
    MissingBiuClock,
    MissingCiuClock,
    MissingReset,
    MissingSysreg,
    HardwareEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MmcBringUpPlan {
    pub host : MmcHostDescription,
    pub blockers : Vec<MmcActivationBlocker>,
}

impl MmcBringUpPlan {
    /// Hardware activation remains deliberately unavailable until board
    /// clock/reset/pinmux/card and controller behavior are verified.
    pub const fn can_activate(&self) -> bool { false }
}

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

pub fn bring_up_plan(host : &MmcHostDescription) -> MmcBringUpPlan {
    let mut blockers = Vec::new();
    if host.mmio.base == 0 || host.mmio.base % 4 != 0 || host.mmio.size < 0x100 {
        blockers.push(MmcActivationBlocker::InvalidMmio);
    }
    if host.irq == 0 {
        blockers.push(MmcActivationBlocker::InvalidIrq);
    }
    if !matches!(host.bus_width, 1 | 4 | 8) {
        blockers.push(MmcActivationBlocker::InvalidBusWidth);
    }
    if host.biu_clock.provider == 0 || host.biu_clock.args.is_empty() {
        blockers.push(MmcActivationBlocker::MissingBiuClock);
    }
    if host.ciu_clock.provider == 0 || host.ciu_clock.args.is_empty() {
        blockers.push(MmcActivationBlocker::MissingCiuClock);
    }
    if host.reset.provider == 0 || host.reset.args.is_empty() {
        blockers.push(MmcActivationBlocker::MissingReset);
    }
    if host.sysreg.is_none() {
        blockers.push(MmcActivationBlocker::MissingSysreg);
    }
    blockers.push(MmcActivationBlocker::HardwareEvidence);
    MmcBringUpPlan { host : host.clone(), blockers }
}

#[cfg(test)]
mod tests {
    use alloc::vec;
    use super::*;

    fn host() -> MmcHostDescription {
        MmcHostDescription { mmio : MmioRegion { base : 0x1602_0000, size : 0x1000 },
                              irq : 91,
                              bus_width : 4,
                              max_frequency_hz : Some(50_000_000),
                              fifo_depth : Some(32),
                              non_removable : false,
                              biu_clock : ResourceSpecifier { provider : 1, args : vec![0] },
                              ciu_clock : ResourceSpecifier { provider : 2, args : vec![1] },
                              reset : ResourceSpecifier { provider : 3, args : vec![0] },
                              sysreg : Some(SysregField { provider : 4,
                                                          offset : 0x10,
                                                          shift : 0,
                                                          mask : 0x3 }) }
    }

    #[test]
    fn valid_topology_still_requires_hardware_evidence() {
        let plan = bring_up_plan(&host());
        assert_eq!(plan.blockers, vec![MmcActivationBlocker::HardwareEvidence]);
        assert!(!plan.can_activate());
    }

    #[test]
    fn malformed_resources_are_reported_without_mmio() {
        let mut value = host();
        value.mmio.base = 0;
        value.bus_width = 2;
        value.biu_clock.provider = 0;
        value.sysreg = None;
        let plan = bring_up_plan(&value);
        assert!(plan.blockers.contains(&MmcActivationBlocker::InvalidMmio));
        assert!(plan.blockers.contains(&MmcActivationBlocker::InvalidBusWidth));
        assert!(plan.blockers.contains(&MmcActivationBlocker::MissingBiuClock));
        assert!(plan.blockers.contains(&MmcActivationBlocker::MissingSysreg));
        assert!(!plan.can_activate());
    }
}

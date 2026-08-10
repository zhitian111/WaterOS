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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MmcControllerConfig {
    pub target_frequency_hz : u32,
    pub fifo_depth : u32,
    pub bus_width : u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmcConfigError {
    InvalidStaticResources,
    MissingTargetFrequency,
    MissingFifoDepth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmcInitializationError {
    NotReady,
    Core(MmcError),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MmcHardwareEvidence {
    pub clock_verified : bool,
    pub reset_verified : bool,
    pub irq_verified : bool,
    pub card_path_verified : bool,
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

    /// Evaluate externally supplied board evidence without performing any
    /// register access. This is the only path that may eventually clear the
    /// static `HardwareEvidence` blocker.
    pub fn activation_ready(&self, evidence : MmcHardwareEvidence) -> bool {
        self.blockers.len() == 1 &&
        self.blockers[0] == MmcActivationBlocker::HardwareEvidence &&
        evidence.clock_verified && evidence.reset_verified && evidence.irq_verified &&
        evidence.card_path_verified
    }

    /// Produce only protocol/controller parameters; this does not touch
    /// clocks, reset, pinmux, power or MMIO.
    pub fn controller_config(&self) -> Result<MmcControllerConfig, MmcConfigError> {
        if self.blockers.iter().any(|blocker| {
            !matches!(blocker, MmcActivationBlocker::HardwareEvidence)
        }) {
            return Err(MmcConfigError::InvalidStaticResources);
        }
        let target_frequency_hz = self.host.max_frequency_hz.ok_or(
            MmcConfigError::MissingTargetFrequency)?;
        let fifo_depth = self.host.fifo_depth.ok_or(MmcConfigError::MissingFifoDepth)?;
        Ok(MmcControllerConfig { target_frequency_hz,
                                  fifo_depth,
                                  bus_width : self.host.bus_width })
    }
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

/// Construct and initialize the shared controller only after explicit board
/// evidence has been supplied. This function is not called during generic
/// machine bring-up and does not register a block device.
pub fn initialize_controller<R : RegisterIo>(plan : &MmcBringUpPlan,
                                              evidence : MmcHardwareEvidence,
                                              registers : R,
                                              input_frequency_hz : u32,
                                              poll_limit : usize)
                                              -> Result<DwMmc<R>, MmcInitializationError> {
    if !plan.activation_ready(evidence) {
        return Err(MmcInitializationError::NotReady);
    }
    let config = plan.controller_config().map_err(|_| MmcInitializationError::NotReady)?;
    let mut controller = DwMmc::probe(registers, poll_limit)
        .map_err(MmcInitializationError::Core)?;
    controller.initialize_polling_with_bus_width(input_frequency_hz,
                                                 config.target_frequency_hz,
                                                 config.fifo_depth,
                                                 config.bus_width)
               .map_err(MmcInitializationError::Core)?;
    Ok(controller)
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
        assert!(!plan.activation_ready(MmcHardwareEvidence::default()));
        assert!(plan.activation_ready(MmcHardwareEvidence { clock_verified : true,
                                                            reset_verified : true,
                                                            irq_verified : true,
                                                            card_path_verified : true }));
        assert_eq!(plan.controller_config(),
                   Ok(MmcControllerConfig { target_frequency_hz : 50_000_000,
                                             fifo_depth : 32,
                                             bus_width : 4 }));
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
        assert_eq!(plan.controller_config(), Err(MmcConfigError::InvalidStaticResources));
    }

    struct Registers { values : [u32; 32] }

    impl RegisterIo for Registers {
        fn read32(&mut self, offset : usize) -> Result<u32, MmcError> {
            if offset == 0x000 || offset == 0x02c || offset == 0x044 {
                return Ok(0);
            }
            self.values.get(offset / 4).copied().ok_or(MmcError::RegisterOutOfRange)
        }

        fn write32(&mut self, offset : usize, value : u32) -> Result<(), MmcError> {
            *self.values.get_mut(offset / 4).ok_or(MmcError::RegisterOutOfRange)? = value;
            Ok(())
        }
    }

    #[test]
    fn gated_initializer_consumes_controller_config_only_with_evidence() {
        let plan = bring_up_plan(&host());
        let evidence = MmcHardwareEvidence { clock_verified : true,
                                              reset_verified : true,
                                              irq_verified : true,
                                              card_path_verified : true };
        let initialized = initialize_controller(&plan,
                                                evidence,
                                                Registers { values : [0; 32] },
                                                50_000_000,
                                                4);
        assert!(initialized.is_ok(), "initializer failed: {:?}", initialized.err());
        assert!(matches!(initialize_controller(&plan,
                                                MmcHardwareEvidence::default(),
                                                Registers { values: [0; 32] },
                                                50_000_000,
                                                4),
                         Err(MmcInitializationError::NotReady)));
    }
}

//! Deferred 2K1000LA MMC bring-up planning.
//!
//! The SD protocol in [`dw_mmc::sd`] is reusable, but the 2K1000LA DTB exposes
//! a 0x68-byte controller window plus a separate auxiliary window. That does
//! not match [`dw_mmc::mmc::MmioRegisters`], whose versioned FIFO is addressed
//! at controller offset 0x100 or 0x200. Until the split register layout is
//! confirmed from vendor documentation and physical reads, this module must
//! not construct or touch a real [`dw_mmc::mmc::DwMmc`] instance.

use crate::topology::MmcDescription;
use api_v0::MmioRegion;

/// Minimum documented main-register window from the upstream 2K1000 DTS.
const MIN_CONTROLLER_WINDOW : usize = 0x68;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanError {
    ControllerWindowTooSmall,
    MissingAuxiliaryWindow,
    MissingClock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationBlocker {
    SplitRegisterLayoutUnverified,
    InputClockRateUnknown,
    ClockControlUnavailable,
    FifoDepthUnknown,
    PowerSequencingUnavailable,
    CardDetectUnavailable,
}

/// Validated resource snapshot for future conservative PIO activation.
///
/// This is deliberately not convertible to `DwMmc`: constructing that host is
/// unsafe until the auxiliary/FIFO mapping and clock prerequisites are known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BringUpPlan {
    pub controller_mmio : MmioRegion,
    pub auxiliary_mmio : MmioRegion,
    pub bus_width : u8,
    pub blockers : [ActivationBlocker; 6],
}

impl BringUpPlan {
    pub const fn can_activate(&self) -> bool { false }
}

pub fn plan(description : &MmcDescription) -> Result<BringUpPlan, PlanError> {
    if description.controller_mmio.size < MIN_CONTROLLER_WINDOW {
        return Err(PlanError::ControllerWindowTooSmall);
    }
    let auxiliary_mmio = description.auxiliary_mmio
                                    .filter(|region| region.size >= 4)
                                    .ok_or(PlanError::MissingAuxiliaryWindow)?;
    if description.clocks.len() != 1 {
        return Err(PlanError::MissingClock);
    }
    Ok(BringUpPlan {
        controller_mmio : description.controller_mmio,
        auxiliary_mmio,
        bus_width : description.bus_width,
        blockers : [ActivationBlocker::SplitRegisterLayoutUnverified,
                    ActivationBlocker::InputClockRateUnknown,
                    ActivationBlocker::ClockControlUnavailable,
                    ActivationBlocker::FifoDepthUnknown,
                    ActivationBlocker::PowerSequencingUnavailable,
                    ActivationBlocker::CardDetectUnavailable],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topology::{CardDetect, InterruptSpec, NamedResource, ResourceSpecifier};
    use alloc::vec;

    fn description() -> MmcDescription {
        MmcDescription {
            controller_mmio : MmioRegion { base : 0x1fe2_c000, size : 0x68 },
            auxiliary_mmio : Some(MmioRegion { base : 0x1fe0_0438, size : 8 }),
            interrupt : InterruptSpec { parent_phandle : 1,
                                        cells : [31, 4, 0, 0],
                                        cell_count : 2 },
            clocks : vec![NamedResource {
                name : None,
                specifier : ResourceSpecifier { provider_phandle : 2, args : vec![0] },
            }],
            dma : None,
            bus_width : 4,
            card_detect : CardDetect::NonRemovable,
            vmmc_supply : None,
            vqmmc_supply : None,
        }
    }

    #[test]
    fn preserves_split_windows_but_refuses_activation() {
        let plan = plan(&description()).unwrap();
        assert_eq!(plan.controller_mmio.size, 0x68);
        assert_eq!(plan.auxiliary_mmio.base, 0x1fe0_0438);
        assert_eq!(plan.bus_width, 4);
        assert!(!plan.can_activate());
        assert!(plan.blockers.contains(&ActivationBlocker::SplitRegisterLayoutUnverified));
    }

    #[test]
    fn rejects_resources_that_cannot_back_future_register_io() {
        let mut value = description();
        value.auxiliary_mmio = None;
        assert_eq!(plan(&value), Err(PlanError::MissingAuxiliaryWindow));

        let mut value = description();
        value.controller_mmio.size = 0x64;
        assert_eq!(plan(&value), Err(PlanError::ControllerWindowTooSmall));

        let mut value = description();
        value.clocks.clear();
        assert_eq!(plan(&value), Err(PlanError::MissingClock));
    }
}

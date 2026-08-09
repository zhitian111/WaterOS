//! CPU hardware-line to LIOINTC bank resolution.
//!
//! The CPUINTC hardware line is architecture evidence only. This module binds
//! it to exactly one topology-described LIOINTC and derives the global bank by
//! ascending main-MMIO address. Reading ESTAT and servicing volatile LIOINTC
//! registers remains `UNVERIFIED_ON_HARDWARE`.

use crate::{irq_domain::MAX_BANKS, topology::BoardTopology};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParentLineError {
    InvalidLine,
    MissingController,
    AmbiguousController,
    DuplicateMmio,
    TooManyBanks,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParentLineBinding {
    pub bank : usize,
    pub main_mmio : usize,
}

/// Resolve one CPUINTC HWI line (0..7) to a stable LIOINTC bank.
pub fn resolve_parent_line(topology : &BoardTopology,
                           hardware_line : u32)
                           -> Result<ParentLineBinding, ParentLineError> {
    if hardware_line >= 8 { return Err(ParentLineError::InvalidLine); }
    let controllers = &topology.interrupt_controllers;
    if controllers.len() > MAX_BANKS { return Err(ParentLineError::TooManyBanks); }
    for (index, left) in controllers.iter().enumerate() {
        for right in &controllers[index + 1..] {
            if left.main_mmio.base == right.main_mmio.base {
                return Err(ParentLineError::DuplicateMmio);
            }
        }
    }
    let mut matching = controllers.iter().filter(|controller| {
        controller.parent_interrupts.iter().flatten().any(|parent| {
            parent.cell_count > 0 && parent.cells[0] == hardware_line
        })
    });
    let controller = matching.next().ok_or(ParentLineError::MissingController)?;
    if matching.next().is_some() { return Err(ParentLineError::AmbiguousController); }
    let bank = controllers.iter()
                          .filter(|candidate| candidate.main_mmio.base < controller.main_mmio.base)
                          .count();
    Ok(ParentLineBinding { bank, main_mmio : controller.main_mmio.base })
}

#[cfg(test)]
mod tests {
    use alloc::vec;
    use api_v0::MmioRegion;

    use super::*;
    use crate::topology::{BoardTopology, InterruptControllerDescription, InterruptSpec};

    fn controller(base : usize, line : u32) -> InterruptControllerDescription {
        let mut parents = core::array::from_fn(|_| None);
        parents[0] = Some(InterruptSpec { parent_phandle : 99,
                                          cells : [line, 0, 0, 0],
                                          cell_count : 1 });
        InterruptControllerDescription {
            phandle : Some(base as u32),
            main_mmio : MmioRegion { base, size : 0x40 },
            core_isr : vec![MmioRegion { base : base - 0x400, size : 8 }],
            interrupt_cells : 2,
            parent_interrupts : parents,
            parent_source_maps : [0, 0, 0, 0],
        }
    }

    fn topology(controllers : alloc::vec::Vec<InterruptControllerDescription>)
                -> BoardTopology {
        BoardTopology { uarts : vec![], interrupt_controllers : controllers,
                        mmc_hosts : vec![], dma_controllers : vec![] }
    }

    #[test]
    fn parent_line_bank_is_stable_across_discovery_order() {
        let low = controller(0x1fe0_1400, 2);
        let high = controller(0x1fe0_1440, 3);
        let expected = ParentLineBinding { bank : 1, main_mmio : 0x1fe0_1440 };
        assert_eq!(resolve_parent_line(&topology(vec![low.clone(), high.clone()]), 3),
                   Ok(expected));
        assert_eq!(resolve_parent_line(&topology(vec![high, low]), 3), Ok(expected));
    }

    #[test]
    fn rejects_missing_ambiguous_and_invalid_parent_lines() {
        assert_eq!(resolve_parent_line(&topology(vec![controller(0x1000, 2)]), 3),
                   Err(ParentLineError::MissingController));
        assert_eq!(resolve_parent_line(&topology(vec![controller(0x1000, 2),
                                                       controller(0x1040, 2)]), 2),
                   Err(ParentLineError::AmbiguousController));
        assert_eq!(resolve_parent_line(&topology(vec![]), 8),
                   Err(ParentLineError::InvalidLine));
    }
}

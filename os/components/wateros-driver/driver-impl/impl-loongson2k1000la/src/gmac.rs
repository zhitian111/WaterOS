//! Loongson PCI-GMAC activation contract.
//!
//! This module deliberately contains no MMIO access. It turns DTB discovery
//! plus independently verified hardware evidence into a fail-closed decision.

use alloc::vec::Vec;

use crate::topology::NetworkDescription;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GmacBlocker {
    InvalidPciFunction,
    MissingInterrupt,
    InterruptNamesMismatch,
    MissingPhyMode,
    MissingPhyHandle,
    BarNotAssigned,
    DmaNotVerified,
    IrqRouteNotVerified,
    PhyLinkNotVerified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GmacActivationEvidence {
    pub bar_assigned : bool,
    pub dma_verified : bool,
    pub irq_route_verified : bool,
    pub phy_link_verified : bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GmacActivationPlan {
    pub bus : u8,
    pub device : u8,
    pub function : u8,
    pub interrupt_count : u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GmacActivation {
    Deferred(Vec<GmacBlocker>),
    Ready(GmacActivationPlan),
}

pub fn evaluate(description : &NetworkDescription,
                evidence : GmacActivationEvidence)
                -> GmacActivation {
    let mut blockers = Vec::new();
    if description.device != 3 || description.function > 1 {
        blockers.push(GmacBlocker::InvalidPciFunction);
    }
    if description.interrupts.is_empty() {
        blockers.push(GmacBlocker::MissingInterrupt);
    }
    if !description.interrupt_names.is_empty() &&
       description.interrupt_names.len() != description.interrupts.len()
    {
        blockers.push(GmacBlocker::InterruptNamesMismatch);
    }
    if description.phy_mode.is_none() {
        blockers.push(GmacBlocker::MissingPhyMode);
    }
    if description.phy_handle.is_none() {
        blockers.push(GmacBlocker::MissingPhyHandle);
    }
    if !evidence.bar_assigned { blockers.push(GmacBlocker::BarNotAssigned); }
    if !evidence.dma_verified { blockers.push(GmacBlocker::DmaNotVerified); }
    if !evidence.irq_route_verified { blockers.push(GmacBlocker::IrqRouteNotVerified); }
    if !evidence.phy_link_verified { blockers.push(GmacBlocker::PhyLinkNotVerified); }
    if blockers.is_empty() {
        GmacActivation::Ready(GmacActivationPlan { bus : description.bus,
                                                   device : description.device,
                                                   function : description.function,
                                                   interrupt_count : description.interrupts.len() as u8 })
    } else {
        GmacActivation::Deferred(blockers)
    }
}

#[cfg(test)]
mod tests {
    use alloc::{string::String, vec};
    use super::*;
    use crate::topology::InterruptSpec;

    fn description() -> NetworkDescription {
        NetworkDescription { bus : 0, device : 3, function : 0,
                              interrupts : vec![InterruptSpec { parent_phandle : 1,
                                                                 cells : [1, 0, 0, 0],
                                                                 cell_count : 2 };
                                                 2],
                              interrupt_names : vec![String::from("macirq"),
                                                     String::from("eth_lpi")],
                              phy_mode : Some(String::from("rgmii-id")),
                              phy_handle : Some(7) }
    }

    #[test]
    fn default_evidence_is_explicitly_deferred() {
        let result = evaluate(&description(), GmacActivationEvidence::default());
        let GmacActivation::Deferred(blockers) = result else { panic!("unexpected ready") };
        assert!(blockers.contains(&GmacBlocker::BarNotAssigned));
        assert!(blockers.contains(&GmacBlocker::DmaNotVerified));
        assert!(blockers.contains(&GmacBlocker::PhyLinkNotVerified));
    }

    #[test]
    fn complete_evidence_produces_copyable_plan() {
        let result = evaluate(&description(), GmacActivationEvidence {
            bar_assigned : true, dma_verified : true,
            irq_route_verified : true, phy_link_verified : true,
        });
        assert_eq!(result, GmacActivation::Ready(GmacActivationPlan {
            bus : 0, device : 3, function : 0, interrupt_count : 2,
        }));
    }

    #[test]
    fn malformed_dtb_metadata_never_becomes_ready() {
        let mut malformed = description();
        malformed.function = 2;
        malformed.interrupt_names.pop();
        let result = evaluate(&malformed, GmacActivationEvidence {
            bar_assigned : true, dma_verified : true,
            irq_route_verified : true, phy_link_verified : true,
        });
        let GmacActivation::Deferred(blockers) = result else { panic!("unexpected ready") };
        assert_eq!(blockers, vec![GmacBlocker::InvalidPciFunction,
                                  GmacBlocker::InterruptNamesMismatch]);
    }
}

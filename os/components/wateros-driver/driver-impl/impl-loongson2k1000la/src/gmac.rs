//! Loongson PCI-GMAC activation contract.
//!
//! This module deliberately contains no MMIO access. It turns DTB discovery
//! plus independently verified hardware evidence into a fail-closed decision.

use alloc::vec::Vec;

use crate::pci::{bar_is_assigned, PciConfigSnapshot};
use crate::topology::NetworkDescription;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GmacBlocker {
    PciIdentityNotVerified,
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
    pub pci_identity_verified : bool,
    pub bar_assigned : bool,
    pub dma_verified : bool,
    pub irq_route_verified : bool,
    pub phy_link_verified : bool,
}

impl GmacActivationEvidence {
    /// Complete board evidence after PCI identity/BAR, DMA, IRQ routing and
    /// PHY link checks have been performed by a platform-specific layer.
    pub const fn complete() -> Self {
        Self { pci_identity_verified : true,
               bar_assigned : true,
               dma_verified : true,
               irq_route_verified : true,
               phy_link_verified : true }
    }

    pub const fn is_complete(self) -> bool {
        self.pci_identity_verified && self.bar_assigned && self.dma_verified &&
        self.irq_route_verified && self.phy_link_verified
    }
}

/// Convert read-only PCI evidence into the portion of the GMAC activation
/// contract that can be proven without touching BARs, DMA or PHY registers.
///
/// `expected_vendor`/`expected_device` are optional because some firmware
/// descriptions omit a stable PCI identity. Omitting either never weakens the
/// class-code check, and all runtime datapath evidence remains false.
pub fn evidence_from_pci_snapshot(snapshot : &PciConfigSnapshot,
                                  expected_vendor : Option<u16>,
                                  expected_device : Option<u16>)
                                  -> GmacActivationEvidence {
    let vendor_ok = expected_vendor.map_or(true,
                                           |vendor| snapshot.identity.vendor_id == vendor);
    let device_ok = expected_device.map_or(true,
                                           |device| snapshot.identity.device_id == device);
    let identity_verified = snapshot.identity.class_code == 0x02 && vendor_ok && device_ok;
    let bar_assigned = snapshot.bar_error.is_none() &&
                       snapshot.bars.iter().copied().flatten().any(|bar| {
                           bar_is_assigned(Ok(bar))
                       });
    GmacActivationEvidence { pci_identity_verified : identity_verified,
                             bar_assigned,
                             ..GmacActivationEvidence::default() }
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
    if !evidence.pci_identity_verified {
        blockers.push(GmacBlocker::PciIdentityNotVerified);
    }
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
        let evidence = GmacActivationEvidence::complete();
        assert!(evidence.is_complete());
        let result = evaluate(&description(), evidence);
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
            pci_identity_verified : true,
            bar_assigned : true, dma_verified : true,
            irq_route_verified : true, phy_link_verified : true,
        });
        let GmacActivation::Deferred(blockers) = result else { panic!("unexpected ready") };
        assert_eq!(blockers, vec![GmacBlocker::InvalidPciFunction,
                                  GmacBlocker::InterruptNamesMismatch]);
    }

    #[test]
    fn pci_snapshot_only_proves_identity_and_bar() {
        let snapshot = crate::pci::PciConfigSnapshot {
            identity : crate::pci::PciIdentity { location : crate::pci::PciLocation {
                                                    bus : 0, device : 3, function : 0,
                                                },
                                                vendor_id : 0x0014,
                                                device_id : 0x1000,
                                                class_code : 0x02,
                                                subclass : 0,
                                                prog_if : 0 },
            bars : [Some(crate::pci::PciBar::Memory32 { index : 0,
                                                        base : 0x8000_0000,
                                                        prefetchable : false }); 6],
            bar_error : None,
        };
        let evidence = evidence_from_pci_snapshot(&snapshot, Some(0x0014), Some(0x1000));
        assert!(evidence.pci_identity_verified);
        assert!(evidence.bar_assigned);
        assert!(!evidence.dma_verified);
        assert!(!evidence.phy_link_verified);
    }

    #[test]
    fn pci_snapshot_mismatch_or_bar_error_stays_deferred() {
        let snapshot = crate::pci::PciConfigSnapshot {
            identity : crate::pci::PciIdentity { location : crate::pci::PciLocation {
                                                    bus : 0, device : 3, function : 0,
                                                },
                                                vendor_id : 0x1234,
                                                device_id : 0x5678,
                                                class_code : 0x02,
                                                subclass : 0,
                                                prog_if : 0 },
            bars : [None; 6],
            bar_error : Some(crate::pci::PciBarError::UnsupportedMemoryType),
        };
        let evidence = evidence_from_pci_snapshot(&snapshot, Some(0x0014), None);
        assert!(!evidence.pci_identity_verified);
        assert!(!evidence.bar_assigned);
    }

    #[test]
    fn every_missing_runtime_fact_retains_its_blocker() {
        let complete = GmacActivationEvidence::complete();
        let cases = [
            (GmacActivationEvidence { pci_identity_verified : false, ..complete },
             GmacBlocker::PciIdentityNotVerified),
            (GmacActivationEvidence { bar_assigned : false, ..complete },
             GmacBlocker::BarNotAssigned),
            (GmacActivationEvidence { dma_verified : false, ..complete },
             GmacBlocker::DmaNotVerified),
            (GmacActivationEvidence { irq_route_verified : false, ..complete },
             GmacBlocker::IrqRouteNotVerified),
            (GmacActivationEvidence { phy_link_verified : false, ..complete },
             GmacBlocker::PhyLinkNotVerified),
        ];
        for (evidence, blocker) in cases {
            let GmacActivation::Deferred(blockers) = evaluate(&description(), evidence) else {
                panic!("incomplete evidence must remain deferred")
            };
            assert!(blockers.contains(&blocker));
        }
    }
}

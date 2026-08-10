//! Read-only AHCI capability and activation contract for 2K1000LA bring-up.
//!
//! This module intentionally stops before controller MMIO, BAR sizing writes,
//! DMA setup, or port start.  It turns a firmware/PCI snapshot into explicit
//! blockers that can be tested without a board.

use api_v0::MmioRegion;
use crate::pci::{PciBar, PciConfigSnapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AhciPciError {
    ClassMismatch,
    MissingMemoryBar,
    AddressUnrepresentable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AhciSnapshot {
    pub abar : MmioRegion,
    pub version : u32,
    pub ports_implemented : u32,
    pub irq : u32,
}

/// Convert a read-only PCI function snapshot into the minimum AHCI ABAR
/// window.  BAR size probing is deliberately not attempted; callers must
/// retain the `HardwareEvidence` blocker until a board-specific resource
/// window has been verified.
pub fn snapshot_from_pci(snapshot : &PciConfigSnapshot,
                          version : u32,
                          ports_implemented : u32,
                          irq : u32)
                          -> Result<AhciSnapshot, AhciPciError> {
    let identity = snapshot.identity;
    if identity.class_code != 0x01 || identity.subclass != 0x06 || identity.prog_if != 0x01 {
        return Err(AhciPciError::ClassMismatch);
    }
    let base = snapshot.bars.iter().flatten().find_map(|bar| {
        match bar {
            PciBar::Memory32 { base, .. } => Some(u64::from(*base)),
            PciBar::Memory64 { base, .. } => Some(*base),
            PciBar::Io { .. } => None,
        }
    }).ok_or(AhciPciError::MissingMemoryBar)?;
    let base = usize::try_from(base).map_err(|_| AhciPciError::AddressUnrepresentable)?;
    Ok(AhciSnapshot { abar : MmioRegion { base, size : 0x100 },
                      version, ports_implemented, irq })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AhciBlocker {
    InvalidAbar,
    AbarSizeUnverified,
    InvalidVersion,
    NoImplementedPorts,
    MissingIrq,
    DmaUnverified,
    HardwareEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AhciHardwareEvidence {
    pub abar_size_verified : bool,
    pub dma_verified : bool,
    pub irq_verified : bool,
    pub link_verified : bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AhciActivationPlan {
    pub snapshot : AhciSnapshot,
    pub blockers : alloc::vec::Vec<AhciBlocker>,
}

impl AhciActivationPlan {
    /// AHCI remains opt-in until a board-specific MMIO/DMA sequence is proven.
    pub const fn can_activate(&self) -> bool { false }

    pub fn evidence_ready(&self, evidence : AhciHardwareEvidence) -> bool {
        self.blockers.len() == 3 &&
        self.blockers.contains(&AhciBlocker::AbarSizeUnverified) &&
        self.blockers.contains(&AhciBlocker::DmaUnverified) &&
        self.blockers.contains(&AhciBlocker::HardwareEvidence) &&
        evidence.abar_size_verified && evidence.dma_verified && evidence.irq_verified &&
        evidence.link_verified
    }
}

pub fn diagnose(snapshot : AhciSnapshot) -> AhciActivationPlan {
    let mut blockers = alloc::vec::Vec::new();
    if snapshot.abar.base == 0 || snapshot.abar.base % 0x1000 != 0 ||
       snapshot.abar.size < 0x100 {
        blockers.push(AhciBlocker::InvalidAbar);
    }
    // PCI BAR size probing is intentionally not implemented. A minimal
    // diagnostic window must never be treated as the controller's real size.
    blockers.push(AhciBlocker::AbarSizeUnverified);
    // AHCI 1.x+ is encoded as major in bits 23..16 and minor in 15..0.
    if snapshot.version == 0 || snapshot.version >> 16 == 0 {
        blockers.push(AhciBlocker::InvalidVersion);
    }
    if snapshot.ports_implemented == 0 {
        blockers.push(AhciBlocker::NoImplementedPorts);
    }
    if snapshot.irq == 0 {
        blockers.push(AhciBlocker::MissingIrq);
    }
    blockers.push(AhciBlocker::DmaUnverified);
    blockers.push(AhciBlocker::HardwareEvidence);
    AhciActivationPlan { snapshot, blockers }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pci::{PciBar, PciConfigSnapshot, PciIdentity, PciLocation};

    fn snapshot() -> AhciSnapshot {
        AhciSnapshot { abar : MmioRegion { base : 0x1fe8_0000, size : 0x1000 },
                        version : 0x0001_0300,
                        ports_implemented : 1,
                        irq : 42 }
    }

    fn pci_snapshot(class_code : u8, bar : Option<PciBar>) -> PciConfigSnapshot {
        let mut bars = [None; 6];
        bars[0] = bar;
        PciConfigSnapshot { identity : PciIdentity { location : PciLocation { bus : 0, device : 3, function : 0 },
                                                     vendor_id : 0x0014, device_id : 0x1000,
                                                     class_code, subclass : 0x06, prog_if : 0x01 },
                            bars, bar_error : None }
    }

    #[test]
    fn pci_snapshot_requires_ahci_class_and_memory_bar() {
        let snapshot = pci_snapshot(0x01, Some(PciBar::Memory32 { index : 0,
                                                                    base : 0x1fe8_0000,
                                                                    prefetchable : false }));
        let ahci = snapshot_from_pci(&snapshot, 0x0001_0300, 1, 42).unwrap();
        assert_eq!(ahci.abar, MmioRegion { base : 0x1fe8_0000, size : 0x100 });
        assert_eq!(snapshot_from_pci(&pci_snapshot(0x02, snapshot.bars[0]), 1, 1, 42),
                   Err(AhciPciError::ClassMismatch));
        assert_eq!(snapshot_from_pci(&pci_snapshot(0x01, None), 1, 1, 42),
                   Err(AhciPciError::MissingMemoryBar));
    }

    #[test]
    fn pci_snapshot_accepts_64_bit_memory_bar() {
        let snapshot = pci_snapshot(0x01, Some(PciBar::Memory64 { index : 0,
                                                                    base : 0x0000_0001_1fe8_0000,
                                                                    prefetchable : true }));
        let ahci = snapshot_from_pci(&snapshot, 1, 1, 42).unwrap();
        assert_eq!(ahci.abar.base, 0x0000_0001_1fe8_0000usize);
    }

    #[test]
    fn valid_snapshot_stays_deferred_without_board_evidence() {
        let plan = diagnose(snapshot());
        assert_eq!(plan.blockers,
                   alloc::vec![AhciBlocker::AbarSizeUnverified,
                               AhciBlocker::DmaUnverified,
                               AhciBlocker::HardwareEvidence]);
        assert!(!plan.can_activate());
        assert!(!plan.evidence_ready(AhciHardwareEvidence::default()));
        assert!(plan.evidence_ready(AhciHardwareEvidence { abar_size_verified : true,
                                                           dma_verified : true,
                                                           irq_verified : true,
                                                           link_verified : true }));
    }

    #[test]
    fn malformed_snapshot_reports_all_static_blockers() {
        let plan = diagnose(AhciSnapshot { abar : MmioRegion { base : 0x1002, size : 4 },
                                           version : 0,
                                           ports_implemented : 0,
                                           irq : 0 });
        assert!(plan.blockers.contains(&AhciBlocker::InvalidAbar));
        assert!(plan.blockers.contains(&AhciBlocker::InvalidVersion));
        assert!(plan.blockers.contains(&AhciBlocker::NoImplementedPorts));
        assert!(plan.blockers.contains(&AhciBlocker::MissingIrq));
        assert!(!plan.evidence_ready(AhciHardwareEvidence { abar_size_verified : true,
                                                             dma_verified : true,
                                                             irq_verified : true,
                                                             link_verified : true }));
    }
}

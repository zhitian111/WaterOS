//! Read-only AHCI capability and activation contract for 2K1000LA bring-up.
//!
//! This module intentionally stops before controller MMIO, BAR sizing writes,
//! DMA setup, or port start.  It turns a firmware/PCI snapshot into explicit
//! blockers that can be tested without a board.

use api_v0::MmioRegion;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AhciSnapshot {
    pub abar : MmioRegion,
    pub version : u32,
    pub ports_implemented : u32,
    pub irq : u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AhciBlocker {
    InvalidAbar,
    InvalidVersion,
    NoImplementedPorts,
    MissingIrq,
    DmaUnverified,
    HardwareEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AhciHardwareEvidence {
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
        self.blockers.len() == 2 &&
        self.blockers.contains(&AhciBlocker::DmaUnverified) &&
        self.blockers.contains(&AhciBlocker::HardwareEvidence) &&
        evidence.dma_verified && evidence.irq_verified && evidence.link_verified
    }
}

pub fn diagnose(snapshot : AhciSnapshot) -> AhciActivationPlan {
    let mut blockers = alloc::vec::Vec::new();
    if snapshot.abar.base == 0 || snapshot.abar.base % 0x1000 != 0 ||
       snapshot.abar.size < 0x100 {
        blockers.push(AhciBlocker::InvalidAbar);
    }
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

    fn snapshot() -> AhciSnapshot {
        AhciSnapshot { abar : MmioRegion { base : 0x1fe8_0000, size : 0x1000 },
                        version : 0x0001_0300,
                        ports_implemented : 1,
                        irq : 42 }
    }

    #[test]
    fn valid_snapshot_stays_deferred_without_board_evidence() {
        let plan = diagnose(snapshot());
        assert_eq!(plan.blockers,
                   alloc::vec![AhciBlocker::DmaUnverified, AhciBlocker::HardwareEvidence]);
        assert!(!plan.can_activate());
        assert!(!plan.evidence_ready(AhciHardwareEvidence::default()));
        assert!(plan.evidence_ready(AhciHardwareEvidence { dma_verified : true,
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
        assert!(!plan.evidence_ready(AhciHardwareEvidence { dma_verified : true,
                                                             irq_verified : true,
                                                             link_verified : true }));
    }
}

//! Concrete board-level IRQ owner variants.
//!
//! MMC can produce rearm evidence from its documented W1C status register.
//! APBDMA remains deliberately one-shot/keep-masked until its device-side IRQ
//! clear semantics are known and tested on hardware.

use crate::{irq_domain::{AcknowledgedIrq, GlobalIrq, IrqDisposition},
            irq_owner::IrqOwner,
            mmc::{MmcIrqAckError, RegisterIo, acknowledge_interrupt}};

pub struct MmcCommandOwner<R> {
    expected_irq : GlobalIrq,
    registers : R,
    handled : u64,
    last_error : Option<MmcIrqAckError>,
}

impl<R> MmcCommandOwner<R> {
    pub const fn new(expected_irq : GlobalIrq, registers : R) -> Self {
        Self { expected_irq, registers, handled : 0, last_error : None }
    }

    pub const fn handled(&self) -> u64 { self.handled }
    pub const fn last_error(&self) -> Option<MmcIrqAckError> { self.last_error }
    pub fn registers(&self) -> &R { &self.registers }
    pub fn registers_mut(&mut self) -> &mut R { &mut self.registers }
    pub fn into_registers(self) -> R { self.registers }
}

impl<R : RegisterIo> IrqOwner for MmcCommandOwner<R> {
    fn handle(&mut self, acknowledged : AcknowledgedIrq) -> IrqDisposition {
        self.handled = self.handled.saturating_add(1);
        match acknowledge_interrupt(&mut self.registers, self.expected_irq, acknowledged) {
            Ok(disposition) => {
                self.last_error = None;
                disposition
            }
            Err(failure) => {
                self.last_error = Some(failure.error);
                IrqDisposition::KeepMasked
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DeferredApbDmaOwner {
    handled : u64,
    last_irq : Option<GlobalIrq>,
}

impl DeferredApbDmaOwner {
    pub const fn handled(&self) -> u64 { self.handled }
    pub const fn last_irq(&self) -> Option<GlobalIrq> { self.last_irq }
}

impl IrqOwner for DeferredApbDmaOwner {
    fn handle(&mut self, acknowledged : AcknowledgedIrq) -> IrqDisposition {
        self.handled = self.handled.saturating_add(1);
        self.last_irq = Some(acknowledged.irq());
        IrqDisposition::KeepMasked
    }
}

pub enum BoardIrqOwner<R> {
    MmcCommand(MmcCommandOwner<R>),
    ApbDmaDeferred(DeferredApbDmaOwner),
}

impl<R : RegisterIo> IrqOwner for BoardIrqOwner<R> {
    fn handle(&mut self, acknowledged : AcknowledgedIrq) -> IrqDisposition {
        match self {
            Self::MmcCommand(owner) => owner.handle(acknowledged),
            Self::ApbDmaDeferred(owner) => owner.handle(acknowledged),
        }
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;
    use dw_mmc::mmc::MmcError;

    use super::*;
    use crate::{irq_domain::DeviceAckedIrq, irq_owner::IrqOwnerTable};

    const REG_INT : usize = 0x3c;
    const COMMAND_SENT : u32 = 1 << 6;

    #[derive(Default)]
    struct MockRegisters {
        status : u32,
        writes : Vec<(usize, u32)>,
    }

    impl RegisterIo for MockRegisters {
        fn read32(&mut self, offset : usize) -> Result<u32, MmcError> {
            if offset == REG_INT { Ok(self.status) } else { Err(MmcError::RegisterOutOfRange) }
        }
        fn write32(&mut self, offset : usize, value : u32) -> Result<(), MmcError> {
            self.writes.push((offset, value));
            Ok(())
        }
    }

    fn acknowledged(irq : GlobalIrq) -> AcknowledgedIrq {
        AcknowledgedIrq::after_mask_ack(irq)
    }

    #[test]
    fn mmc_owner_persists_state_through_owner_table() {
        let irq = GlobalIrq::from_bank_local(0, 31).unwrap();
        let owner = BoardIrqOwner::MmcCommand(MmcCommandOwner::new(
            irq, MockRegisters { status : COMMAND_SENT, writes : Vec::new() }));
        let mut table = IrqOwnerTable::new();
        table.register(irq, owner).unwrap_or_else(|_| panic!("register failed"));
        let active = table.begin(acknowledged(irq)).unwrap();
        let (active, disposition) = active.handle();
        assert_eq!(disposition,
                   IrqDisposition::Rearm(DeviceAckedIrq::after_device_clear(irq)));
        table.finish(active).unwrap_or_else(|_| panic!("finish failed"));
        let BoardIrqOwner::MmcCommand(owner) = table.get(irq).unwrap() else {
            panic!("wrong owner variant")
        };
        assert_eq!(owner.handled(), 1);
        assert_eq!(owner.last_error(), None);
        assert_eq!(owner.registers().writes, [(REG_INT, COMMAND_SENT)]);
    }

    #[test]
    fn board_owners_keep_failed_mmc_and_apbdma_masked() {
        let mmc_irq = GlobalIrq::from_bank_local(0, 31).unwrap();
        let mut mmc = BoardIrqOwner::MmcCommand(MmcCommandOwner::new(
            mmc_irq, MockRegisters::default()));
        assert_eq!(mmc.handle(acknowledged(mmc_irq)), IrqDisposition::KeepMasked);
        let BoardIrqOwner::MmcCommand(owner) = &mmc else { unreachable!() };
        assert_eq!(owner.last_error(), Some(MmcIrqAckError::NoKnownPending));

        let dma_irq = GlobalIrq::from_bank_local(1, 13).unwrap();
        let mut dma : BoardIrqOwner<MockRegisters> =
            BoardIrqOwner::ApbDmaDeferred(DeferredApbDmaOwner::default());
        assert_eq!(dma.handle(acknowledged(dma_irq)), IrqDisposition::KeepMasked);
        let BoardIrqOwner::ApbDmaDeferred(owner) = dma else { unreachable!() };
        assert_eq!(owner.handled(), 1);
        assert_eq!(owner.last_irq(), Some(dma_irq));
    }
}

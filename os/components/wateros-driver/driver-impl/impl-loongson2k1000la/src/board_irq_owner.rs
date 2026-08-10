//! Concrete board-level IRQ owner variants.
//!
//! MMC can produce rearm evidence from its documented W1C status register.
//! APBDMA remains deliberately one-shot/keep-masked until its device-side IRQ
//! clear semantics are known and tested on hardware.

use crate::{irq_domain::{AcknowledgedIrq, GlobalIrq, IrqDisposition},
            irq_owner::IrqOwner,
            mmc::{MmcIrqAckError, RegisterIo, acknowledge_interrupt_observed}};
use core::num::NonZeroU64;

/// Software-only identity for one armed MMC/APBDMA read transaction.
///
/// `UNVERIFIED_ON_HARDWARE`: neither interrupt source carries this value. It
/// prevents software cross-generation mixing but cannot classify a physically
/// late IRQ after hardware has been rearmed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadTransactionId(NonZeroU64);

impl ReadTransactionId {
    pub const fn new(raw : u64) -> Option<Self> {
        match NonZeroU64::new(raw) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    pub const fn raw(self) -> u64 { self.0.get() }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadTransactionSequenceError {
    Exhausted,
}

pub struct ReadTransactionSequence {
    next : Option<ReadTransactionId>,
}

impl ReadTransactionSequence {
    pub const fn new() -> Self {
        Self { next : ReadTransactionId::new(1) }
    }

    #[cfg(test)]
    const fn starting_at(raw : u64) -> Self {
        Self { next : ReadTransactionId::new(raw) }
    }

    pub fn allocate(&mut self) -> Result<ReadTransactionId, ReadTransactionSequenceError> {
        let current = self.next.ok_or(ReadTransactionSequenceError::Exhausted)?;
        self.next = current.raw().checked_add(1).and_then(ReadTransactionId::new);
        Ok(current)
    }
}

impl Default for ReadTransactionSequence {
    fn default() -> Self { Self::new() }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadIrqOwnerError {
    AlreadyArmed,
    NotArmed,
    PendingNotConsumed,
    WrongTransaction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadIrqOwnerBinding {
    Armed(ReadTransactionId),
    Pending(ReadTransactionId),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MmcReadIrqReceipt {
    pub transaction : ReadTransactionId,
    pub interrupts : u32,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ApbDmaReadIrqReceipt {
    pub transaction : ReadTransactionId,
    pub acknowledged : AcknowledgedIrq,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadIrqPairError {
    WrongTransaction,
    DuplicateMmc,
    DuplicateDma,
}

pub struct ReadIrqReceiptFailure<T> {
    pub error : ReadIrqPairError,
    pub receipt : T,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadPairOwner {
    Mmc,
    Dma,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadPairOwnerFailure {
    pub owner : ReadPairOwner,
    pub error : ReadIrqOwnerError,
}

#[derive(Debug)]
pub struct DrainedReadIrqs {
    pub mmc : Option<MmcReadIrqReceipt>,
    pub dma : Option<ApbDmaReadIrqReceipt>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct ArmedReadIrqs {
    transaction : ReadTransactionId,
}

impl ArmedReadIrqs {
    pub const fn transaction(&self) -> ReadTransactionId { self.transaction }
}

/// Exclusive pre-start reservation for both read interrupt owners.
/// Dropping without commit rolls the software arms back.
#[must_use = "commit the read IRQ arms or let the guard roll them back"]
pub struct ReadIrqArmGuard<'a, R> {
    mmc : &'a mut MmcCommandOwner<R>,
    dma : &'a mut DeferredApbDmaOwner,
    transaction : ReadTransactionId,
    committed : bool,
}

impl<'a, R> ReadIrqArmGuard<'a, R> {
    pub fn arm(mmc : &'a mut MmcCommandOwner<R>,
               dma : &'a mut DeferredApbDmaOwner,
               transaction : ReadTransactionId)
               -> Result<Self, ReadPairOwnerFailure> {
        arm_read_owners(mmc, dma, transaction)?;
        Ok(Self { mmc, dma, transaction, committed : false })
    }

    pub fn commit(mut self) -> ArmedReadIrqs {
        self.committed = true;
        ArmedReadIrqs { transaction : self.transaction }
    }
}

impl<R> Drop for ReadIrqArmGuard<'_, R> {
    fn drop(&mut self) {
        if self.committed { return; }
        // The guard holds both exclusive owner borrows, so neither runtime IRQ
        // service nor receipt production can occur between arm and this drop.
        if self.mmc.read_binding() == Some(ReadIrqOwnerBinding::Armed(self.transaction)) &&
           self.dma.read_binding() == Some(ReadIrqOwnerBinding::Armed(self.transaction))
        {
            self.mmc.read_armed = None;
            self.dma.armed = None;
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadIrqReservationError {
    Runtime(crate::irq_runtime::RuntimeError),
    MmcOwnerVariant,
    DmaOwnerVariant,
    Arm(ReadPairOwnerFailure),
}

/// Reserve two runtime owner slots and arm the expected MMC/APBDMA variants.
pub fn reserve_read_irq_owners<'a, I, R>(
    runtime : &'a mut crate::irq_runtime::BoardIrqRuntime<I, BoardIrqOwner<R>>,
    mmc_irq : GlobalIrq,
    dma_irq : GlobalIrq,
    transaction : ReadTransactionId)
    -> Result<ReadIrqArmGuard<'a, R>, ReadIrqReservationError>
where I : crate::liointc::RegisterIo
{
    let (mmc, dma) = runtime.owners_mut(mmc_irq, dma_irq)
                             .map_err(ReadIrqReservationError::Runtime)?;
    let BoardIrqOwner::MmcCommand(mmc) = mmc else {
        return Err(ReadIrqReservationError::MmcOwnerVariant);
    };
    let BoardIrqOwner::ApbDmaDeferred(dma) = dma else {
        return Err(ReadIrqReservationError::DmaOwnerVariant);
    };
    ReadIrqArmGuard::arm(mmc, dma, transaction)
        .map_err(ReadIrqReservationError::Arm)
}

/// Arm both read IRQ owners as one software transaction. A DMA-side failure
/// rolls the newly armed MMC owner back before returning.
pub fn arm_read_owners<R>(mmc : &mut MmcCommandOwner<R>,
                          dma : &mut DeferredApbDmaOwner,
                          transaction : ReadTransactionId)
                          -> Result<(), ReadPairOwnerFailure> {
    mmc.arm_read(transaction)
       .map_err(|error| ReadPairOwnerFailure { owner : ReadPairOwner::Mmc, error })?;
    if let Err(error) = dma.arm_read(transaction) {
        mmc.read_armed = None;
        return Err(ReadPairOwnerFailure { owner : ReadPairOwner::Dma, error });
    }
    Ok(())
}

/// Retire one generation from both owners after first validating both sides.
/// Pending receipts are returned intact for completion or recovery handling.
pub fn drain_read_owners<R>(mmc : &mut MmcCommandOwner<R>,
                            dma : &mut DeferredApbDmaOwner,
                            transaction : ReadTransactionId)
                            -> Result<DrainedReadIrqs, ReadPairOwnerFailure> {
    validate_binding(mmc.read_binding(), transaction)
        .map_err(|error| ReadPairOwnerFailure { owner : ReadPairOwner::Mmc, error })?;
    validate_binding(dma.read_binding(), transaction)
        .map_err(|error| ReadPairOwnerFailure { owner : ReadPairOwner::Dma, error })?;
    mmc.read_armed = None;
    dma.armed = None;
    Ok(DrainedReadIrqs { mmc : mmc.read_pending.take(),
                         dma : dma.pending.take() })
}

fn validate_binding(binding : Option<ReadIrqOwnerBinding>,
                    transaction : ReadTransactionId)
                    -> Result<(), ReadIrqOwnerError> {
    let bound = match binding {
        Some(ReadIrqOwnerBinding::Armed(bound)) |
        Some(ReadIrqOwnerBinding::Pending(bound)) => bound,
        None => return Err(ReadIrqOwnerError::NotArmed),
    };
    if bound == transaction { Ok(()) } else { Err(ReadIrqOwnerError::WrongTransaction) }
}

/// Collect exactly one MMC and one APBDMA receipt from the same software
/// generation before either may be applied to a carrying read session.
pub struct ReadIrqPair {
    transaction : ReadTransactionId,
    mmc : Option<MmcReadIrqReceipt>,
    dma : Option<ApbDmaReadIrqReceipt>,
}

impl ReadIrqPair {
    pub const fn new(transaction : ReadTransactionId) -> Self {
        Self { transaction, mmc : None, dma : None }
    }

    pub fn submit_mmc(&mut self, receipt : MmcReadIrqReceipt)
                      -> Result<(), ReadIrqReceiptFailure<MmcReadIrqReceipt>> {
        let error = if receipt.transaction != self.transaction {
            Some(ReadIrqPairError::WrongTransaction)
        } else if self.mmc.is_some() {
            Some(ReadIrqPairError::DuplicateMmc)
        } else {
            None
        };
        if let Some(error) = error {
            return Err(ReadIrqReceiptFailure { error, receipt });
        }
        self.mmc = Some(receipt);
        Ok(())
    }

    pub fn submit_dma(&mut self, receipt : ApbDmaReadIrqReceipt)
                      -> Result<(), ReadIrqReceiptFailure<ApbDmaReadIrqReceipt>> {
        let error = if receipt.transaction != self.transaction {
            Some(ReadIrqPairError::WrongTransaction)
        } else if self.dma.is_some() {
            Some(ReadIrqPairError::DuplicateDma)
        } else {
            None
        };
        if let Some(error) = error {
            return Err(ReadIrqReceiptFailure { error, receipt });
        }
        self.dma = Some(receipt);
        Ok(())
    }

    pub fn take_ready(&mut self)
                      -> Option<(MmcReadIrqReceipt, ApbDmaReadIrqReceipt)> {
        if self.mmc.is_none() || self.dma.is_none() { return None; }
        Some((self.mmc.take().unwrap(), self.dma.take().unwrap()))
    }
}

pub struct MmcCommandOwner<R> {
    expected_irq : GlobalIrq,
    registers : R,
    handled : u64,
    last_error : Option<MmcIrqAckError>,
    read_armed : Option<ReadTransactionId>,
    read_pending : Option<MmcReadIrqReceipt>,
    last_read_error : Option<ReadIrqOwnerError>,
}

impl<R> MmcCommandOwner<R> {
    pub const fn new(expected_irq : GlobalIrq, registers : R) -> Self {
        Self { expected_irq,
               registers,
               handled : 0,
               last_error : None,
               read_armed : None,
               read_pending : None,
               last_read_error : None }
    }

    pub const fn handled(&self) -> u64 { self.handled }
    pub const fn last_error(&self) -> Option<MmcIrqAckError> { self.last_error }
    pub const fn last_read_error(&self) -> Option<ReadIrqOwnerError> {
        self.last_read_error
    }
    pub fn read_binding(&self) -> Option<ReadIrqOwnerBinding> {
        self.read_pending
            .map(|receipt| ReadIrqOwnerBinding::Pending(receipt.transaction))
            .or_else(|| self.read_armed.map(ReadIrqOwnerBinding::Armed))
    }
    pub fn arm_read(&mut self, transaction : ReadTransactionId)
                    -> Result<(), ReadIrqOwnerError> {
        if self.read_pending.is_some() { return Err(ReadIrqOwnerError::PendingNotConsumed); }
        if self.read_armed.is_some() { return Err(ReadIrqOwnerError::AlreadyArmed); }
        self.read_armed = Some(transaction);
        self.last_read_error = None;
        Ok(())
    }
    pub fn disarm_read(&mut self, transaction : ReadTransactionId)
                       -> Result<(), ReadIrqOwnerError> {
        match self.read_binding() {
            Some(ReadIrqOwnerBinding::Pending(_)) => {
                Err(ReadIrqOwnerError::PendingNotConsumed)
            },
            binding => {
                validate_binding(binding, transaction)?;
                self.read_armed = None;
                Ok(())
            },
        }
    }
    pub fn take_read_receipt(&mut self) -> Option<MmcReadIrqReceipt> {
        self.read_pending.take()
    }
    pub fn registers(&self) -> &R { &self.registers }
    pub fn registers_mut(&mut self) -> &mut R { &mut self.registers }
    pub fn into_registers(self) -> R { self.registers }
}

impl<R : RegisterIo> IrqOwner for MmcCommandOwner<R> {
    fn handle(&mut self, acknowledged : AcknowledgedIrq) -> IrqDisposition {
        self.handled = self.handled.saturating_add(1);
        match acknowledge_interrupt_observed(&mut self.registers,
                                             self.expected_irq,
                                             acknowledged) {
            Ok(receipt) => {
                self.last_error = None;
                if self.read_pending.is_some() {
                    self.last_read_error = Some(ReadIrqOwnerError::PendingNotConsumed);
                    IrqDisposition::KeepMasked
                } else if let Some(transaction) = self.read_armed.take() {
                    self.read_pending = Some(MmcReadIrqReceipt {
                        transaction,
                        interrupts : receipt.interrupts,
                    });
                    self.last_read_error = None;
                    IrqDisposition::KeepMasked
                } else {
                    receipt.disposition
                }
            }
            Err(failure) => {
                self.last_error = Some(failure.error);
                IrqDisposition::KeepMasked
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeferredApbDmaError {
    UnexpectedIrq,
    NotArmed,
    PendingNotConsumed,
}

#[derive(Debug, PartialEq, Eq)]
pub struct DeferredApbDmaOwner {
    expected_irq : GlobalIrq,
    handled : u64,
    armed : Option<ReadTransactionId>,
    pending : Option<ApbDmaReadIrqReceipt>,
    last_error : Option<DeferredApbDmaError>,
}

impl DeferredApbDmaOwner {
    pub const fn new(expected_irq : GlobalIrq) -> Self {
        Self { expected_irq, handled : 0, armed : None, pending : None, last_error : None }
    }

    pub const fn handled(&self) -> u64 { self.handled }
    pub fn pending_irq(&self) -> Option<GlobalIrq> {
        self.pending.as_ref().map(|receipt| receipt.acknowledged.irq())
    }
    pub const fn last_error(&self) -> Option<DeferredApbDmaError> { self.last_error }
    pub fn read_binding(&self) -> Option<ReadIrqOwnerBinding> {
        self.pending.as_ref()
            .map(|receipt| ReadIrqOwnerBinding::Pending(receipt.transaction))
            .or_else(|| self.armed.map(ReadIrqOwnerBinding::Armed))
    }

    pub fn arm_read(&mut self, transaction : ReadTransactionId)
                    -> Result<(), ReadIrqOwnerError> {
        if self.pending.is_some() { return Err(ReadIrqOwnerError::PendingNotConsumed); }
        if self.armed.is_some() { return Err(ReadIrqOwnerError::AlreadyArmed); }
        self.armed = Some(transaction);
        self.last_error = None;
        Ok(())
    }

    pub fn disarm_read(&mut self, transaction : ReadTransactionId)
                       -> Result<(), ReadIrqOwnerError> {
        match self.read_binding() {
            Some(ReadIrqOwnerBinding::Pending(_)) => {
                Err(ReadIrqOwnerError::PendingNotConsumed)
            },
            binding => {
                validate_binding(binding, transaction)?;
                self.armed = None;
                Ok(())
            },
        }
    }

    /// Take the one transaction-bound IRQ receipt retained for the DMA session.
    ///
    /// The source remains masked. Consuming this token does not prove any
    /// descriptor status meaning or permit rearming the interrupt.
    pub fn take_read_receipt(&mut self) -> Option<ApbDmaReadIrqReceipt> {
        self.pending.take()
    }
}

impl IrqOwner for DeferredApbDmaOwner {
    fn handle(&mut self, acknowledged : AcknowledgedIrq) -> IrqDisposition {
        self.handled = self.handled.saturating_add(1);
        if acknowledged.irq() != self.expected_irq {
            self.last_error = Some(DeferredApbDmaError::UnexpectedIrq);
        } else if self.pending.is_some() {
            self.last_error = Some(DeferredApbDmaError::PendingNotConsumed);
        } else if let Some(transaction) = self.armed.take() {
            self.pending = Some(ApbDmaReadIrqReceipt { transaction, acknowledged });
            self.last_error = None;
        } else {
            self.last_error = Some(DeferredApbDmaError::NotArmed);
        }
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

    #[derive(Default)]
    struct ModelLioIo;

    impl crate::liointc::RegisterIo for ModelLioIo {
        fn read32(&self, _address : usize) -> u32 { 0 }
        fn write32(&mut self, _address : usize, _value : u32) {}
        fn write8(&mut self, _address : usize, _value : u8) {}
    }

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

    fn transaction(raw : u64) -> ReadTransactionId { ReadTransactionId::new(raw).unwrap() }

    fn owner_runtime(mmc_irq : GlobalIrq, dma_irq : GlobalIrq, swapped : bool)
        -> crate::irq_runtime::BoardIrqRuntime<ModelLioIo, BoardIrqOwner<MockRegisters>> {
        let bank0 = crate::liointc::LioIntc::new(ModelLioIo, 0, 0x1000, &[0x2000]).unwrap();
        let bank1 = crate::liointc::LioIntc::new(ModelLioIo, 1, 0x1040, &[0x2040]).unwrap();
        let mut runtime = crate::irq_runtime::BoardIrqRuntime::new(
            [Some(bank0), Some(bank1)], [None; 8]).unwrap();
        let mmc = BoardIrqOwner::MmcCommand(
            MmcCommandOwner::new(mmc_irq, MockRegisters::default()));
        let dma : BoardIrqOwner<MockRegisters> = BoardIrqOwner::ApbDmaDeferred(
            DeferredApbDmaOwner::new(dma_irq));
        if swapped {
            runtime.register(mmc_irq, dma).unwrap_or_else(|_| panic!("register failed"));
            runtime.register(dma_irq, mmc).unwrap_or_else(|_| panic!("register failed"));
        } else {
            runtime.register(mmc_irq, mmc).unwrap_or_else(|_| panic!("register failed"));
            runtime.register(dma_irq, dma).unwrap_or_else(|_| panic!("register failed"));
        }
        runtime
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
            BoardIrqOwner::ApbDmaDeferred(DeferredApbDmaOwner::new(dma_irq));
        let BoardIrqOwner::ApbDmaDeferred(owner) = &mut dma else { unreachable!() };
        owner.arm_read(transaction(1)).unwrap();
        assert_eq!(dma.handle(acknowledged(dma_irq)), IrqDisposition::KeepMasked);
        let BoardIrqOwner::ApbDmaDeferred(mut owner) = dma else { unreachable!() };
        assert_eq!(owner.handled(), 1);
        assert_eq!(owner.pending_irq(), Some(dma_irq));
        assert_eq!(owner.last_error(), None);
        let receipt = owner.take_read_receipt().unwrap();
        assert_eq!(receipt.transaction, transaction(1));
        assert_eq!(receipt.acknowledged.irq(), dma_irq);
        assert_eq!(owner.pending_irq(), None);
    }

    #[test]
    fn apbdma_owner_preserves_first_token_across_wrong_and_duplicate_irqs() {
        let dma_irq = GlobalIrq::from_bank_local(1, 13).unwrap();
        let wrong_irq = GlobalIrq::from_bank_local(0, 13).unwrap();
        let mut owner = DeferredApbDmaOwner::new(dma_irq);
        owner.arm_read(transaction(7)).unwrap();
        assert_eq!(owner.handle(acknowledged(wrong_irq)), IrqDisposition::KeepMasked);
        assert_eq!(owner.pending_irq(), None);
        assert_eq!(owner.last_error(), Some(DeferredApbDmaError::UnexpectedIrq));

        assert_eq!(owner.handle(acknowledged(dma_irq)), IrqDisposition::KeepMasked);
        assert_eq!(owner.pending_irq(), Some(dma_irq));
        assert_eq!(owner.handle(acknowledged(dma_irq)), IrqDisposition::KeepMasked);
        assert_eq!(owner.pending_irq(), Some(dma_irq));
        assert_eq!(owner.last_error(), Some(DeferredApbDmaError::PendingNotConsumed));
        let receipt = owner.take_read_receipt().unwrap();
        assert_eq!(receipt.transaction, transaction(7));
        assert_eq!(receipt.acknowledged.irq(), dma_irq);
        assert_eq!(owner.take_read_receipt(), None);
        assert_eq!(owner.handled(), 3);
    }

    #[test]
    fn read_transaction_sequence_never_emits_zero_or_wraps() {
        assert_eq!(ReadTransactionId::new(0), None);
        let mut sequence = ReadTransactionSequence::new();
        assert_eq!(sequence.allocate().unwrap().raw(), 1);
        assert_eq!(sequence.allocate().unwrap().raw(), 2);

        let mut sequence = ReadTransactionSequence::starting_at(u64::MAX);
        assert_eq!(sequence.allocate().unwrap().raw(), u64::MAX);
        assert_eq!(sequence.allocate(), Err(ReadTransactionSequenceError::Exhausted));
    }

    #[test]
    fn read_irq_pair_rejects_stale_and_duplicate_receipts_in_both_orders() {
        let current = transaction(11);
        let stale = transaction(10);
        let mmc_irq = GlobalIrq::from_bank_local(0, 31).unwrap();
        let dma_irq = GlobalIrq::from_bank_local(1, 13).unwrap();
        let mut mmc = MmcCommandOwner::new(
            mmc_irq,
            MockRegisters { status : COMMAND_SENT | 1, writes : Vec::new() });
        let mut dma = DeferredApbDmaOwner::new(dma_irq);
        mmc.arm_read(current).unwrap();
        dma.arm_read(current).unwrap();
        assert_eq!(mmc.handle(acknowledged(mmc_irq)), IrqDisposition::KeepMasked);
        assert_eq!(dma.handle(acknowledged(dma_irq)), IrqDisposition::KeepMasked);
        let mmc_receipt = mmc.take_read_receipt().unwrap();
        let dma_receipt = dma.take_read_receipt().unwrap();
        assert_eq!(mmc_receipt.interrupts, COMMAND_SENT | 1);

        let mut pair = ReadIrqPair::new(current);
        let failure = pair.submit_mmc(MmcReadIrqReceipt {
                              transaction : stale,
                              interrupts : COMMAND_SENT,
                          }).unwrap_err();
        assert_eq!(failure.error, ReadIrqPairError::WrongTransaction);
        pair.submit_dma(dma_receipt).unwrap_or_else(|_| panic!("DMA receipt rejected"));
        assert!(pair.take_ready().is_none());
        pair.submit_mmc(mmc_receipt).unwrap_or_else(|_| panic!("MMC receipt rejected"));
        let (mmc_receipt, dma_receipt) = pair.take_ready().unwrap();
        assert_eq!(mmc_receipt.transaction, current);
        assert_eq!(dma_receipt.transaction, current);

        pair.submit_mmc(mmc_receipt).unwrap_or_else(|_| panic!("MMC receipt rejected"));
        let duplicate = pair.submit_mmc(mmc_receipt).unwrap_err();
        assert_eq!(duplicate.error, ReadIrqPairError::DuplicateMmc);
        pair.submit_dma(dma_receipt).unwrap_or_else(|_| panic!("DMA receipt rejected"));
        assert!(pair.take_ready().is_some());
    }

    #[test]
    fn read_owners_refuse_rearm_until_each_generation_is_consumed() {
        let first = transaction(41);
        let second = transaction(42);
        let mmc_irq = GlobalIrq::from_bank_local(0, 31).unwrap();
        let dma_irq = GlobalIrq::from_bank_local(1, 13).unwrap();
        let mut mmc = MmcCommandOwner::new(
            mmc_irq,
            MockRegisters { status : COMMAND_SENT, writes : Vec::new() });
        assert_eq!(mmc.arm_read(first), Ok(()));
        assert_eq!(mmc.arm_read(second), Err(ReadIrqOwnerError::AlreadyArmed));
        assert_eq!(mmc.handle(acknowledged(mmc_irq)), IrqDisposition::KeepMasked);
        assert_eq!(mmc.arm_read(second), Err(ReadIrqOwnerError::PendingNotConsumed));
        assert_eq!(mmc.handle(acknowledged(mmc_irq)), IrqDisposition::KeepMasked);
        assert_eq!(mmc.last_read_error(), Some(ReadIrqOwnerError::PendingNotConsumed));
        assert_eq!(mmc.take_read_receipt().unwrap().transaction, first);
        assert_eq!(mmc.arm_read(second), Ok(()));

        let mut dma = DeferredApbDmaOwner::new(dma_irq);
        assert_eq!(dma.handle(acknowledged(dma_irq)), IrqDisposition::KeepMasked);
        assert_eq!(dma.last_error(), Some(DeferredApbDmaError::NotArmed));
        assert_eq!(dma.arm_read(first), Ok(()));
        assert_eq!(dma.arm_read(second), Err(ReadIrqOwnerError::AlreadyArmed));
        assert_eq!(dma.handle(acknowledged(dma_irq)), IrqDisposition::KeepMasked);
        assert_eq!(dma.arm_read(second), Err(ReadIrqOwnerError::PendingNotConsumed));
        assert_eq!(dma.take_read_receipt().unwrap().transaction, first);
        assert_eq!(dma.arm_read(second), Ok(()));
    }

    #[test]
    fn pair_arm_rolls_mmc_back_when_dma_is_already_bound() {
        let current = transaction(51);
        let occupied = transaction(50);
        let mmc_irq = GlobalIrq::from_bank_local(0, 31).unwrap();
        let dma_irq = GlobalIrq::from_bank_local(1, 13).unwrap();
        let mut mmc = MmcCommandOwner::new(mmc_irq, MockRegisters::default());
        let mut dma = DeferredApbDmaOwner::new(dma_irq);
        dma.arm_read(occupied).unwrap();
        assert_eq!(arm_read_owners(&mut mmc, &mut dma, current),
                   Err(ReadPairOwnerFailure {
                       owner : ReadPairOwner::Dma,
                       error : ReadIrqOwnerError::AlreadyArmed,
                   }));
        assert_eq!(mmc.read_binding(), None);
        assert_eq!(dma.read_binding(), Some(ReadIrqOwnerBinding::Armed(occupied)));
        mmc.arm_read(current).unwrap();
        assert_eq!(drain_read_owners(&mut mmc, &mut dma, current).unwrap_err(),
                   ReadPairOwnerFailure {
                       owner : ReadPairOwner::Dma,
                       error : ReadIrqOwnerError::WrongTransaction,
                   });
        assert_eq!(mmc.read_binding(), Some(ReadIrqOwnerBinding::Armed(current)));
        assert_eq!(dma.read_binding(), Some(ReadIrqOwnerBinding::Armed(occupied)));
        assert_eq!(mmc.disarm_read(current), Ok(()));
        assert_eq!(dma.disarm_read(current), Err(ReadIrqOwnerError::WrongTransaction));
        assert_eq!(dma.disarm_read(occupied), Ok(()));
    }

    #[test]
    fn pair_drain_retires_armed_generation_without_inventing_receipts() {
        let current = transaction(61);
        let mmc_irq = GlobalIrq::from_bank_local(0, 31).unwrap();
        let dma_irq = GlobalIrq::from_bank_local(1, 13).unwrap();
        let mut mmc = MmcCommandOwner::new(mmc_irq, MockRegisters::default());
        let mut dma = DeferredApbDmaOwner::new(dma_irq);
        arm_read_owners(&mut mmc, &mut dma, current).unwrap();
        let drained = drain_read_owners(&mut mmc, &mut dma, current)
            .unwrap_or_else(|_| panic!("matching armed generation did not drain"));
        assert_eq!(drained.mmc, None);
        assert_eq!(drained.dma, None);
        assert_eq!(mmc.read_binding(), None);
        assert_eq!(dma.read_binding(), None);
        assert_eq!(drain_read_owners(&mut mmc, &mut dma, current).unwrap_err(),
                   ReadPairOwnerFailure {
                       owner : ReadPairOwner::Mmc,
                       error : ReadIrqOwnerError::NotArmed,
                   });
    }

    #[test]
    fn pair_drain_prevalidates_generation_and_returns_pending_tokens() {
        let current = transaction(71);
        let wrong = transaction(72);
        let mmc_irq = GlobalIrq::from_bank_local(0, 31).unwrap();
        let dma_irq = GlobalIrq::from_bank_local(1, 13).unwrap();
        let mut mmc = MmcCommandOwner::new(
            mmc_irq,
            MockRegisters { status : COMMAND_SENT | 1, writes : Vec::new() });
        let mut dma = DeferredApbDmaOwner::new(dma_irq);
        arm_read_owners(&mut mmc, &mut dma, current).unwrap();
        assert_eq!(dma.handle(acknowledged(dma_irq)), IrqDisposition::KeepMasked);
        assert_eq!(dma.disarm_read(current), Err(ReadIrqOwnerError::PendingNotConsumed));
        assert_eq!(drain_read_owners(&mut mmc, &mut dma, wrong).unwrap_err(),
                   ReadPairOwnerFailure {
                       owner : ReadPairOwner::Mmc,
                       error : ReadIrqOwnerError::WrongTransaction,
                   });
        assert_eq!(mmc.read_binding(), Some(ReadIrqOwnerBinding::Armed(current)));
        assert_eq!(dma.read_binding(), Some(ReadIrqOwnerBinding::Pending(current)));

        let drained = drain_read_owners(&mut mmc, &mut dma, current)
            .unwrap_or_else(|_| panic!("matching pending generation did not drain"));
        assert_eq!(drained.mmc, None);
        let dma_receipt = drained.dma.expect("pending DMA token was lost");
        assert_eq!(dma_receipt.transaction, current);
        assert_eq!(dma_receipt.acknowledged.irq(), dma_irq);
        assert_eq!(mmc.read_binding(), None);
        assert_eq!(dma.read_binding(), None);

        arm_read_owners(&mut mmc, &mut dma, wrong).unwrap();
        assert_eq!(mmc.handle(acknowledged(mmc_irq)), IrqDisposition::KeepMasked);
        assert_eq!(dma.handle(acknowledged(dma_irq)), IrqDisposition::KeepMasked);
        let drained = drain_read_owners(&mut mmc, &mut dma, wrong)
            .unwrap_or_else(|_| panic!("dual pending generation did not drain"));
        assert_eq!(drained.mmc.unwrap().transaction, wrong);
        assert_eq!(drained.dma.unwrap().transaction, wrong);
    }

    #[test]
    fn runtime_arm_guard_rolls_back_on_drop_and_commit_retains_both_slots() {
        let mmc_irq = GlobalIrq::from_bank_local(0, 31).unwrap();
        let dma_irq = GlobalIrq::from_bank_local(1, 13).unwrap();
        let first = transaction(81);
        let second = transaction(82);
        let mut runtime = owner_runtime(mmc_irq, dma_irq, false);
        {
            let _guard = reserve_read_irq_owners(&mut runtime, mmc_irq, dma_irq, first)
                .unwrap_or_else(|_| panic!("guard reservation failed"));
        }
        let BoardIrqOwner::MmcCommand(mmc) = runtime.owner(mmc_irq).unwrap() else {
            panic!("wrong MMC variant")
        };
        assert_eq!(mmc.read_binding(), None);
        let BoardIrqOwner::ApbDmaDeferred(dma) = runtime.owner(dma_irq).unwrap() else {
            panic!("wrong DMA variant")
        };
        assert_eq!(dma.read_binding(), None);

        let armed = reserve_read_irq_owners(&mut runtime, mmc_irq, dma_irq, second)
            .unwrap_or_else(|_| panic!("guard reservation failed"))
            .commit();
        assert_eq!(armed.transaction(), second);
        let (mmc, dma) = runtime.owners_mut(mmc_irq, dma_irq).unwrap();
        let BoardIrqOwner::MmcCommand(mmc) = mmc else { panic!("wrong MMC variant") };
        let BoardIrqOwner::ApbDmaDeferred(dma) = dma else { panic!("wrong DMA variant") };
        let drained = drain_read_owners(mmc, dma, second)
            .unwrap_or_else(|_| panic!("committed generation did not drain"));
        assert!(drained.mmc.is_none());
        assert!(drained.dma.is_none());
    }

    #[test]
    fn runtime_guard_rejects_same_slot_and_swapped_owner_variants() {
        let mmc_irq = GlobalIrq::from_bank_local(0, 31).unwrap();
        let dma_irq = GlobalIrq::from_bank_local(1, 13).unwrap();
        let current = transaction(91);
        let mut runtime = owner_runtime(mmc_irq, dma_irq, false);
        assert!(matches!(reserve_read_irq_owners(&mut runtime,
                                                 mmc_irq,
                                                 mmc_irq,
                                                 current),
                         Err(ReadIrqReservationError::Runtime(
                             crate::irq_runtime::RuntimeError::Owner(
                                 crate::irq_owner::OwnerError::SameSlot)))));

        let mut runtime = owner_runtime(mmc_irq, dma_irq, true);
        assert!(matches!(reserve_read_irq_owners(&mut runtime,
                                                 mmc_irq,
                                                 dma_irq,
                                                 current),
                         Err(ReadIrqReservationError::MmcOwnerVariant)));
        let BoardIrqOwner::ApbDmaDeferred(owner) = runtime.owner(mmc_irq).unwrap() else {
            panic!("swapped owner changed")
        };
        assert_eq!(owner.read_binding(), None);
    }
}

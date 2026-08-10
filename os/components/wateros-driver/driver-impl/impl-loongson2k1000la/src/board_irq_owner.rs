//! Concrete board-level IRQ owner variants.
//!
//! MMC can produce rearm evidence from its documented W1C status register.
//! APBDMA remains deliberately one-shot/keep-masked until its device-side IRQ
//! clear semantics are known and tested on hardware.

use crate::{irq_domain::{AcknowledgedIrq, GlobalIrq, IrqDisposition},
            irq_owner::IrqOwner,
            mmc::{MmcIrqAckError, RegisterIo, acknowledge_interrupt_observed,
                  clear_masked_interrupt_snapshot, read_interrupt_snapshot_terminal}};
use api_v0::{DriverError, dma::DmaCoherency};
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
pub enum MmcReadRecheckError {
    Owner(ReadIrqOwnerError),
    Ack(MmcIrqAckError),
}

#[derive(Debug, PartialEq, Eq)]
pub struct BoundedMmcReadRecheck {
    transaction : ReadTransactionId,
    remaining : u16,
    polls_completed : u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoundedMmcReadRecheckError {
    InvalidBudget,
    Runtime(crate::irq_runtime::RuntimeError),
    OwnerVariant,
    Binding(Option<ReadIrqOwnerBinding>),
    Recheck(MmcReadRecheckError),
}

#[derive(Debug, PartialEq, Eq)]
pub enum BoundedMmcReadRecheckProgress {
    Pending(BoundedMmcReadRecheck),
    Terminal {
        transaction : ReadTransactionId,
        polls_completed : u16,
    },
    Timeout {
        transaction : ReadTransactionId,
        polls_completed : u16,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoundedMmcReadRecheckStep {
    Pending,
    Terminal,
    Timeout,
}

#[derive(Debug, PartialEq, Eq)]
#[must_use = "recover the bounded recheck and retry or enter read recovery"]
pub struct BoundedMmcReadRecheckFailure {
    pub error : BoundedMmcReadRecheckError,
    pub recheck : BoundedMmcReadRecheck,
}

impl BoundedMmcReadRecheckFailure {
    pub const fn recovery_cause(&self) -> Option<ReadRecoveryCause> {
        match self.error {
            BoundedMmcReadRecheckError::Recheck(error) =>
                Some(ReadRecoveryCause::RecheckFault {
                    error,
                    polls_completed : self.recheck.polls_completed,
                    remaining : self.recheck.remaining,
                }),
            _ => None,
        }
    }
}

impl BoundedMmcReadRecheck {
    pub const fn new(transaction : ReadTransactionId,
                     poll_budget : u16) -> Result<Self, BoundedMmcReadRecheckError> {
        if poll_budget == 0 { return Err(BoundedMmcReadRecheckError::InvalidBudget); }
        Ok(Self { transaction, remaining : poll_budget, polls_completed : 0 })
    }

    pub const fn transaction(&self) -> ReadTransactionId { self.transaction }
    pub const fn remaining(&self) -> u16 { self.remaining }
    pub const fn polls_completed(&self) -> u16 { self.polls_completed }

    /// Execute one serialized masked-status sample. No loop, delay or rearm is
    /// performed here; Pending must be scheduled explicitly by the caller.
    pub fn step<I, R>(mut self,
                      runtime : &mut crate::irq_runtime::BoardIrqRuntime<
                          I, BoardIrqOwner<R>>,
                      mmc_irq : GlobalIrq)
                      -> Result<BoundedMmcReadRecheckProgress,
                                BoundedMmcReadRecheckFailure>
    where I : crate::liointc::RegisterIo, R : RegisterIo
    {
        match self.step_in_place(runtime, mmc_irq) {
            Ok(BoundedMmcReadRecheckStep::Pending) =>
                Ok(BoundedMmcReadRecheckProgress::Pending(self)),
            Ok(BoundedMmcReadRecheckStep::Terminal) =>
                Ok(BoundedMmcReadRecheckProgress::Terminal {
                    transaction : self.transaction,
                    polls_completed : self.polls_completed,
                }),
            Ok(BoundedMmcReadRecheckStep::Timeout) =>
                Ok(BoundedMmcReadRecheckProgress::Timeout {
                    transaction : self.transaction,
                    polls_completed : self.polls_completed,
                }),
            Err(error) => Err(BoundedMmcReadRecheckFailure { error, recheck : self }),
        }
    }

    pub(crate) fn step_in_place<I, R>(
        &mut self,
        runtime : &mut crate::irq_runtime::BoardIrqRuntime<I, BoardIrqOwner<R>>,
        mmc_irq : GlobalIrq)
        -> Result<BoundedMmcReadRecheckStep, BoundedMmcReadRecheckError>
    where I : crate::liointc::RegisterIo, R : RegisterIo
    {
        let owner = match runtime.owner_mut(mmc_irq) {
            Ok(owner) => owner,
            Err(error) => return Err(BoundedMmcReadRecheckError::Runtime(error)),
        };
        let BoardIrqOwner::MmcCommand(owner) = owner else {
            return Err(BoundedMmcReadRecheckError::OwnerVariant);
        };
        match owner.read_binding() {
            Some(ReadIrqOwnerBinding::Pending(transaction))
                if transaction == self.transaction => {
                    return Ok(BoundedMmcReadRecheckStep::Terminal);
                },
            Some(ReadIrqOwnerBinding::Armed(transaction))
                if transaction == self.transaction => {},
            binding => {
                return Err(BoundedMmcReadRecheckError::Binding(binding));
            },
        }
        let result = owner.recheck_masked_read();
        let no_pending = matches!(result,
                                  Err(MmcReadRecheckError::Ack(
                                      MmcIrqAckError::NoKnownPending)));
        if let Err(error) = result {
            if !no_pending {
                return Err(BoundedMmcReadRecheckError::Recheck(error));
            }
        }
        self.remaining -= 1;
        self.polls_completed += 1;
        if result == Ok(true) {
            return Ok(BoundedMmcReadRecheckStep::Terminal);
        }
        if self.remaining == 0 {
            Ok(BoundedMmcReadRecheckStep::Timeout)
        } else {
            Ok(BoundedMmcReadRecheckStep::Pending)
        }
    }
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadRecoveryCause {
    Timeout {
        polls_completed : u16,
    },
    RecheckFault {
        error : MmcReadRecheckError,
        polls_completed : u16,
        remaining : u16,
    },
    CompletionFailure(crate::mmc::ReadCompletionFailure),
}

#[derive(Debug)]
#[must_use = "retain the read recovery evidence for diagnostics"]
pub struct ReadRecoveryReport {
    pub transaction : ReadTransactionId,
    pub cause : ReadRecoveryCause,
    pub partial_mmc_interrupts : u32,
    pub drained : DrainedReadIrqs,
}

#[derive(Debug)]
#[must_use = "apply both same-generation IRQ receipts or retain them for recovery"]
pub struct ReadyReadIrqPair {
    pub mmc : MmcReadIrqReceipt,
    pub dma : ApbDmaReadIrqReceipt,
}

impl ReadyReadIrqPair {
    pub const fn transaction(&self) -> ReadTransactionId { self.mmc.transaction }
}

#[derive(Debug, PartialEq, Eq)]
#[must_use = "bind the armed IRQ generation to a read session or retire it"]
pub struct ArmedReadIrqs {
    transaction : ReadTransactionId,
}

impl ArmedReadIrqs {
    pub const fn transaction(&self) -> ReadTransactionId { self.transaction }

    pub fn bind_prepared_dma<'a, D, P>(
        self,
        session : crate::mmc::PreparedReadDmaSession<'a, D, P>)
        -> IrqArmedReadDmaSession<crate::mmc::PreparedReadDmaSession<'a, D, P>> {
        IrqArmedReadDmaSession { armed : self, session }
    }

    #[cfg(test)]
    pub(crate) const fn fixture(transaction : ReadTransactionId) -> Self {
        Self { transaction }
    }
}

#[derive(Debug, PartialEq, Eq)]
#[must_use = "retire the quiesced read IRQ generation and retain its recovery report"]
pub struct QuiescedReadIrqs {
    armed : ArmedReadIrqs,
}

impl QuiescedReadIrqs {
    pub const fn transaction(&self) -> ReadTransactionId { self.armed.transaction }

    #[cfg(test)]
    pub(crate) const fn fixture(transaction : ReadTransactionId) -> Self {
        Self { armed : ArmedReadIrqs::fixture(transaction) }
    }
}

/// A read DMA typestate carrying the linear software IRQ-generation token.
///
/// `UNVERIFIED_ON_HARDWARE`: the token proves only owner/session ordering. It
/// neither tags physical IRQs nor proves that APBDMA observed a start write.
#[must_use = "advance or explicitly recover the IRQ-armed read DMA session"]
pub struct IrqArmedReadDmaSession<S> {
    armed : ArmedReadIrqs,
    session : S,
}

#[cfg(test)]
pub(crate) struct IrqPairedReadDmaSession<S> {
    pair : ReadyReadIrqPair,
    session : S,
}

#[cfg(test)]
pub(crate) struct ReadSessionPendingPairFailure<S> {
    pub error : ReadPendingPairError,
    pub session : IrqArmedReadDmaSession<S>,
}

#[cfg(test)]
pub(crate) struct ReadSessionTerminalClaimFailure<S> {
    pub error : crate::read_coordinator::ReadTerminalClaimError,
    pub session : IrqArmedReadDmaSession<S>,
}

#[cfg(test)]
pub(crate) struct PairedAcknowledgedReadDmaSession<'a, D, P> {
    pub mmc : MmcReadIrqReceipt,
    pub tracker : crate::mmc::ReadCompletionTracker<
        crate::mmc::AcknowledgedReadDmaSession<'a, D, P>>,
}

#[cfg(test)]
pub(crate) struct PairedQuiescedReadDmaSession<'a, D, P> {
    mmc : MmcReadIrqReceipt,
    tracker : crate::mmc::ReadCompletionTracker<
        crate::mmc::QuiescedReadDmaSession<'a, D, P>>,
}

#[cfg(test)]
pub(crate) struct PairedDmaInspectionFailure<'a, D, P> {
    pub error : crate::apbdma::DescriptorStatusError,
    pub session : PairedAcknowledgedReadDmaSession<'a, D, P>,
}

#[cfg(test)]
pub(crate) enum PairedDmaStatusProgress<'a, D, P> {
    Pending(PairedQuiescedReadDmaSession<'a, D, P>),
    RecoveryRequired {
        mmc : MmcReadIrqReceipt,
        recovery : crate::mmc::ReadCompletionRecovery<
            crate::mmc::QuiescedReadDmaSession<'a, D, P>>,
    },
}

#[cfg(test)]
pub(crate) struct PairedReadDmaIrqFailure<'a, 'e, R, D, P> {
    pub mmc : MmcReadIrqReceipt,
    pub dma_transaction : ReadTransactionId,
    pub failure : crate::mmc::ReadDmaIrqFailure<'a, 'e, R, D, P>,
}

impl<S> IrqArmedReadDmaSession<S> {
    pub const fn transaction(&self) -> ReadTransactionId { self.armed.transaction }
}

pub enum IrqArmedReadDmaStartFailure<'a, 'e, R, D, P> {
    Prepared {
        error : crate::apbdma::ExecutorError,
        read : crate::mmc::DeferredReadPlan,
        session : IrqArmedReadDmaSession<crate::mmc::PreparedReadDmaSession<'a, D, P>>,
    },
    Recovery {
        error : crate::apbdma::ExecutorError,
        read : crate::mmc::DeferredReadPlan,
        session : IrqArmedReadDmaSession<crate::apbdma::RecoverySession<'a, 'e, R, D, P>>,
    },
}

impl<R, D, P> core::fmt::Debug for IrqArmedReadDmaStartFailure<'_, '_, R, D, P> {
    fn fmt(&self, formatter : &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let (state, error, read, transaction) = match self {
            Self::Prepared { error, read, session } =>
                ("Prepared", error, read, session.transaction()),
            Self::Recovery { error, read, session } =>
                ("Recovery", error, read, session.transaction()),
        };
        formatter.debug_struct("IrqArmedReadDmaStartFailure")
                 .field("state", &state)
                 .field("error", error)
                 .field("read", read)
                 .field("transaction", &transaction)
                 .finish()
    }
}

impl<R, D, P> IrqArmedReadDmaStartFailure<'_, '_, R, D, P> {
    pub const fn transaction(&self) -> ReadTransactionId {
        match self {
            Self::Prepared { session, .. } => session.transaction(),
            Self::Recovery { session, .. } => session.transaction(),
        }
    }
}

impl<'a, D : DmaCoherency, P : DmaCoherency>
    IrqArmedReadDmaSession<crate::mmc::PreparedReadDmaSession<'a, D, P>>
{
    pub fn cancel(self)
        -> Result<ArmedReadIrqs, crate::apbdma::SessionFailure<DriverError, Self>> {
        match self.session.cancel() {
            Ok(()) => Ok(self.armed),
            Err(failure) => Err(crate::apbdma::SessionFailure {
                error : failure.error,
                session : Self { armed : self.armed, session : failure.session },
            }),
        }
    }

    pub fn start<'e, R : crate::apbdma::OrderIo>(
        self,
        executor : &'e mut crate::apbdma::Executor<R>)
        -> Result<IrqArmedReadDmaSession<crate::mmc::RunningReadDmaSession<'a, 'e, R, D, P>>,
                  IrqArmedReadDmaStartFailure<'a, 'e, R, D, P>> {
        match self.session.start(executor) {
            Ok(session) => Ok(IrqArmedReadDmaSession { armed : self.armed, session }),
            Err(failure) => {
                let crate::mmc::ReadDmaStartFailure { read, failure } = failure;
                match failure {
                    crate::apbdma::StartSessionFailure::Prepared(failure) => {
                        Err(IrqArmedReadDmaStartFailure::Prepared {
                            error : failure.error,
                            read,
                            session : IrqArmedReadDmaSession {
                                armed : self.armed,
                                session : crate::mmc::PreparedReadDmaSession::from_start_failure(
                                    read, failure.session),
                            },
                        })
                    },
                    crate::apbdma::StartSessionFailure::Recovery(failure) => {
                        Err(IrqArmedReadDmaStartFailure::Recovery {
                            error : failure.error,
                            read,
                            session : IrqArmedReadDmaSession {
                                armed : self.armed, session : failure.session,
                            },
                        })
                    },
                }
            },
        }
    }
}

impl<'a, 'e, R : crate::apbdma::OrderIo, D, P>
    IrqArmedReadDmaSession<crate::apbdma::RecoverySession<'a, 'e, R, D, P>>
{
    pub fn stop(self)
        -> Result<IrqArmedReadDmaSession<crate::apbdma::QuiescedSession<'a, D, P>>,
                  crate::apbdma::SessionFailure<crate::apbdma::ExecutorError, Self>> {
        match self.session.stop() {
            Ok(session) => Ok(IrqArmedReadDmaSession { armed : self.armed, session }),
            Err(failure) => Err(crate::apbdma::SessionFailure {
                error : failure.error,
                session : Self { armed : self.armed, session : failure.session },
            }),
        }
    }
}

#[cfg(test)]
impl<'a, D : DmaCoherency, P> PairedAcknowledgedReadDmaSession<'a, D, P> {
    pub(crate) fn inspect_dma_status<R : crate::apbdma::DescriptorStatusReader,
                                    C : crate::apbdma::DescriptorStatusDecoder>(
        self,
        reader : &mut R,
        decoder : &C)
        -> Result<PairedDmaStatusProgress<'a, D, P>,
                  PairedDmaInspectionFailure<'a, D, P>> {
        let Self { mmc, tracker } = self;
        match tracker.inspect_dma_status(reader, decoder) {
            Ok(crate::mmc::ReadCompletionProgress::Pending(tracker)) =>
                Ok(PairedDmaStatusProgress::Pending(
                    PairedQuiescedReadDmaSession { mmc, tracker })),
            // The paired tracker is freshly constructed immediately before
            // DMA acknowledgement, so it cannot already contain MMC evidence.
            Ok(crate::mmc::ReadCompletionProgress::Completed(_)) =>
                unreachable!("fresh paired tracker completed before MMC receipt"),
            Ok(crate::mmc::ReadCompletionProgress::RecoveryRequired(recovery)) =>
                Ok(PairedDmaStatusProgress::RecoveryRequired { mmc, recovery }),
            Err(failure) => Err(PairedDmaInspectionFailure {
                error : failure.error,
                session : Self { mmc, tracker : failure.tracker },
            }),
        }
    }
}

#[cfg(test)]
impl<'a, D, P> PairedQuiescedReadDmaSession<'a, D, P> {
    /// Consume the terminal MMC snapshot exactly once after DMA is quiesced.
    pub(crate) fn apply_mmc_receipt(
        self)
        -> crate::mmc::ReadCompletionProgress<crate::mmc::QuiescedReadDmaSession<'a, D, P>> {
        self.tracker.terminal_irq_observed(self.mmc.interrupts)
    }
}

impl<'a, 'e, R : crate::apbdma::OrderIo, D, P>
    IrqArmedReadDmaSession<crate::mmc::RunningReadDmaSession<'a, 'e, R, D, P>>
{
    pub const fn plan(&self) -> &crate::mmc::DeferredReadPlan { self.session.plan() }

    pub fn stop(self)
        -> Result<IrqArmedReadDmaSession<crate::apbdma::QuiescedSession<'a, D, P>>,
                  crate::apbdma::SessionFailure<crate::apbdma::ExecutorError, Self>> {
        match self.session.stop() {
            Ok(session) => Ok(IrqArmedReadDmaSession { armed : self.armed, session }),
            Err(failure) => Err(crate::apbdma::SessionFailure {
                error : failure.error,
                session : Self { armed : self.armed, session : failure.session },
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn publish<C : crate::mmc::ReadCommandPublisher>(
        self,
        publisher : &mut C)
        -> Result<IrqArmedReadDmaSession<crate::mmc::PublishedReadDmaSession<'a, 'e, R, D, P>>,
                  crate::mmc::ReadPublishFailure<C::Error, Self>> {
        match self.session.publish(publisher) {
            Ok(session) => Ok(IrqArmedReadDmaSession { armed : self.armed, session }),
            Err(failure) => Err(crate::mmc::ReadPublishFailure {
                error : failure.error,
                session : Self { armed : self.armed, session : failure.session },
            }),
        }
    }
}

impl<'a, D : DmaCoherency, P : DmaCoherency>
    IrqArmedReadDmaSession<crate::apbdma::QuiescedSession<'a, D, P>>
{
    pub fn finish(self)
        -> Result<ArmedReadIrqs,
                  crate::apbdma::SessionFailure<DriverError, Self>> {
        match self.session.finish() {
            Ok(()) => Ok(self.armed),
            Err(failure) => Err(crate::apbdma::SessionFailure {
                error : failure.error,
                session : Self { armed : self.armed, session : failure.session },
            }),
        }
    }

    /// Finish cache ownership recovery and mint the only token accepted by
    /// `retire_quiesced_read_recovery`.
    pub fn finish_recovery(self)
        -> Result<QuiescedReadIrqs,
                  crate::apbdma::SessionFailure<DriverError, Self>> {
        match self.session.finish() {
            Ok(()) => Ok(QuiescedReadIrqs { armed : self.armed }),
            Err(failure) => Err(crate::apbdma::SessionFailure {
                error : failure.error,
                session : Self { armed : self.armed, session : failure.session },
            }),
        }
    }
}

#[cfg(test)]
impl<'a, 'e, R : crate::apbdma::OrderIo, D, P>
    IrqArmedReadDmaSession<crate::mmc::PublishedReadDmaSession<'a, 'e, R, D, P>>
{
    pub(crate) const fn plan(&self) -> &crate::mmc::DeferredReadPlan { self.session.plan() }

    pub(crate) fn take_pending_pair<I, O>(
        self,
        runtime : &mut crate::irq_runtime::BoardIrqRuntime<I, BoardIrqOwner<O>>,
        mmc_irq : GlobalIrq,
        dma_irq : GlobalIrq)
        -> Result<IrqPairedReadDmaSession<crate::mmc::PublishedReadDmaSession<'a, 'e, R, D, P>>,
                  ReadSessionPendingPairFailure<
                      crate::mmc::PublishedReadDmaSession<'a, 'e, R, D, P>>>
    where I : crate::liointc::RegisterIo
    {
        let Self { armed, session } = self;
        match take_pending_read_irq_pair(runtime, mmc_irq, dma_irq, armed) {
            Ok(pair) => Ok(IrqPairedReadDmaSession { pair, session }),
            Err(failure) => Err(ReadSessionPendingPairFailure {
                error : failure.error,
                session : Self { armed : failure.into_armed(), session },
            }),
        }
    }

    pub(crate) fn claim_pending_pair<I, O>(
        self,
        service : crate::read_coordinator::ReadTerminalService<'_>,
        runtime : &mut crate::irq_runtime::BoardIrqRuntime<I, BoardIrqOwner<O>>,
        mmc_irq : GlobalIrq,
        dma_irq : GlobalIrq)
        -> Result<IrqPairedReadDmaSession<crate::mmc::PublishedReadDmaSession<'a, 'e, R, D, P>>,
                  ReadSessionTerminalClaimFailure<
                      crate::mmc::PublishedReadDmaSession<'a, 'e, R, D, P>>>
    where I : crate::liointc::RegisterIo
    {
        let Self { armed, session } = self;
        match service.claim_pair(runtime, mmc_irq, dma_irq, armed) {
            Ok(pair) => Ok(IrqPairedReadDmaSession { pair, session }),
            Err(failure) => Err(ReadSessionTerminalClaimFailure {
                error : failure.error,
                session : Self { armed : failure.into_armed(), session },
            }),
        }
    }

    pub(crate) fn stop(self)
        -> Result<IrqArmedReadDmaSession<crate::apbdma::QuiescedSession<'a, D, P>>,
                  crate::apbdma::SessionFailure<crate::apbdma::ExecutorError, Self>> {
        match self.session.stop() {
            Ok(session) => Ok(IrqArmedReadDmaSession { armed : self.armed, session }),
            Err(failure) => Err(crate::apbdma::SessionFailure {
                error : failure.error,
                session : Self { armed : self.armed, session : failure.session },
            }),
        }
    }
}

#[cfg(test)]
impl<'a, 'e, R : crate::apbdma::OrderIo, D, P>
    IrqPairedReadDmaSession<crate::mmc::PublishedReadDmaSession<'a, 'e, R, D, P>>
{
    pub(crate) fn acknowledge_dma_irq(
        self)
        -> Result<PairedAcknowledgedReadDmaSession<'a, D, P>,
                  PairedReadDmaIrqFailure<'a, 'e, R, D, P>> {
        let Self { pair, session } = self;
        let ReadyReadIrqPair { mmc, dma } = pair;
        let dma_transaction = dma.transaction;
        match session.into_completion_tracker()
                     .acknowledge_dma_irq(dma.acknowledged) {
            Ok(tracker) => Ok(PairedAcknowledgedReadDmaSession { mmc, tracker }),
            Err(failure) => Err(PairedReadDmaIrqFailure {
                mmc, dma_transaction, failure,
            }),
        }
    }
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
            self.mmc.read_interrupts = 0;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadIrqRetireError {
    Runtime(crate::irq_runtime::RuntimeError),
    MmcOwnerVariant,
    DmaOwnerVariant,
    Drain(ReadPairOwnerFailure),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadPendingPairError {
    Runtime(crate::irq_runtime::RuntimeError),
    MmcOwnerVariant,
    DmaOwnerVariant,
    MmcBinding(Option<ReadIrqOwnerBinding>),
    DmaBinding(Option<ReadIrqOwnerBinding>),
    MmcReceiptMissing,
    DmaReceiptMissing,
}

#[must_use = "recover the armed generation and retry after both IRQs are pending"]
pub struct ReadPendingPairFailure {
    pub error : ReadPendingPairError,
    armed : ArmedReadIrqs,
}

impl ReadPendingPairFailure {
    pub fn into_armed(self) -> ArmedReadIrqs { self.armed }
}

#[must_use = "recover the armed generation and retry or keep both IRQs masked"]
pub struct ReadIrqRetireFailure {
    pub error : ReadIrqRetireError,
    armed : ArmedReadIrqs,
}

impl ReadIrqRetireFailure {
    pub fn into_armed(self) -> ArmedReadIrqs { self.armed }
}

#[must_use = "recover the quiesced generation and retry retirement"]
pub struct ReadRecoveryRetireFailure {
    pub error : ReadIrqRetireError,
    pub cause : ReadRecoveryCause,
    quiesced : QuiescedReadIrqs,
}

impl ReadRecoveryRetireFailure {
    pub fn into_quiesced(self) -> QuiescedReadIrqs { self.quiesced }
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

/// Consume an armed generation only after both runtime owner slots validate.
/// This retires software state only; it never rearms either interrupt source.
pub fn retire_read_irq_owners<I, R>(
    runtime : &mut crate::irq_runtime::BoardIrqRuntime<I, BoardIrqOwner<R>>,
    mmc_irq : GlobalIrq,
    dma_irq : GlobalIrq,
    armed : ArmedReadIrqs)
    -> Result<DrainedReadIrqs, ReadIrqRetireFailure>
where I : crate::liointc::RegisterIo
{
    let transaction = armed.transaction;
    let (mmc, dma) = match runtime.owners_mut(mmc_irq, dma_irq) {
        Ok(owners) => owners,
        Err(error) => {
            return Err(ReadIrqRetireFailure {
                error : ReadIrqRetireError::Runtime(error), armed,
            });
        },
    };
    let BoardIrqOwner::MmcCommand(mmc) = mmc else {
        return Err(ReadIrqRetireFailure {
            error : ReadIrqRetireError::MmcOwnerVariant, armed,
        });
    };
    let BoardIrqOwner::ApbDmaDeferred(dma) = dma else {
        return Err(ReadIrqRetireFailure {
            error : ReadIrqRetireError::DmaOwnerVariant, armed,
        });
    };
    drain_read_owners(mmc, dma, transaction).map_err(|error| ReadIrqRetireFailure {
        error : ReadIrqRetireError::Drain(error), armed,
    })
}

/// Capture generation-local partial MMC evidence and retire both owners only
/// after DMA stop and cache recovery produced a `QuiescedReadIrqs` token.
/// This is software-state retirement only; both interrupt sources stay masked.
pub fn retire_quiesced_read_recovery<I, R>(
    runtime : &mut crate::irq_runtime::BoardIrqRuntime<I, BoardIrqOwner<R>>,
    mmc_irq : GlobalIrq,
    dma_irq : GlobalIrq,
    quiesced : QuiescedReadIrqs,
    cause : ReadRecoveryCause)
    -> Result<ReadRecoveryReport, ReadRecoveryRetireFailure>
where I : crate::liointc::RegisterIo
{
    let transaction = quiesced.armed.transaction;
    let (mmc, dma) = match runtime.owners_mut(mmc_irq, dma_irq) {
        Ok(owners) => owners,
        Err(error) => {
            return Err(ReadRecoveryRetireFailure {
                error : ReadIrqRetireError::Runtime(error), cause, quiesced,
            });
        },
    };
    let BoardIrqOwner::MmcCommand(mmc) = mmc else {
        return Err(ReadRecoveryRetireFailure {
            error : ReadIrqRetireError::MmcOwnerVariant, cause, quiesced,
        });
    };
    let BoardIrqOwner::ApbDmaDeferred(dma) = dma else {
        return Err(ReadRecoveryRetireFailure {
            error : ReadIrqRetireError::DmaOwnerVariant, cause, quiesced,
        });
    };
    if let Err(error) = validate_binding(mmc.read_binding(), transaction) {
        return Err(ReadRecoveryRetireFailure {
            error : ReadIrqRetireError::Drain(ReadPairOwnerFailure {
                owner : ReadPairOwner::Mmc, error,
            }),
            cause,
            quiesced,
        });
    }
    if let Err(error) = validate_binding(dma.read_binding(), transaction) {
        return Err(ReadRecoveryRetireFailure {
            error : ReadIrqRetireError::Drain(ReadPairOwnerFailure {
                owner : ReadPairOwner::Dma, error,
            }),
            cause,
            quiesced,
        });
    }
    let partial_mmc_interrupts = mmc.read_interrupts;
    let drained = drain_read_owners(mmc, dma, transaction)
        .expect("bindings were validated before recovery drain");
    Ok(ReadRecoveryReport {
        transaction, cause, partial_mmc_interrupts, drained,
    })
}

/// Atomically take both receipts only when both owners are Pending for the
/// carried generation. Any incomplete or mismatched state is left unchanged.
pub fn take_pending_read_irq_pair<I, R>(
    runtime : &mut crate::irq_runtime::BoardIrqRuntime<I, BoardIrqOwner<R>>,
    mmc_irq : GlobalIrq,
    dma_irq : GlobalIrq,
    armed : ArmedReadIrqs)
    -> Result<ReadyReadIrqPair, ReadPendingPairFailure>
where I : crate::liointc::RegisterIo
{
    let transaction = armed.transaction;
    let (mmc, dma) = match runtime.owners_mut(mmc_irq, dma_irq) {
        Ok(owners) => owners,
        Err(error) => {
            return Err(ReadPendingPairFailure {
                error : ReadPendingPairError::Runtime(error), armed,
            });
        },
    };
    let BoardIrqOwner::MmcCommand(mmc) = mmc else {
        return Err(ReadPendingPairFailure {
            error : ReadPendingPairError::MmcOwnerVariant, armed,
        });
    };
    let BoardIrqOwner::ApbDmaDeferred(dma) = dma else {
        return Err(ReadPendingPairFailure {
            error : ReadPendingPairError::DmaOwnerVariant, armed,
        });
    };
    let expected = Some(ReadIrqOwnerBinding::Pending(transaction));
    let mmc_binding = mmc.read_binding();
    if mmc_binding != expected {
        return Err(ReadPendingPairFailure {
            error : ReadPendingPairError::MmcBinding(mmc_binding), armed,
        });
    }
    let dma_binding = dma.read_binding();
    if dma_binding != expected {
        return Err(ReadPendingPairFailure {
            error : ReadPendingPairError::DmaBinding(dma_binding), armed,
        });
    }
    if mmc.read_pending.is_none() {
        return Err(ReadPendingPairFailure {
            error : ReadPendingPairError::MmcReceiptMissing, armed,
        });
    }
    if dma.pending.is_none() {
        return Err(ReadPendingPairFailure {
            error : ReadPendingPairError::DmaReceiptMissing, armed,
        });
    }
    let mmc_receipt = mmc.read_pending.take().unwrap();
    let dma_receipt = dma.pending.take().unwrap();
    mmc.read_armed = None;
    mmc.read_interrupts = 0;
    dma.armed = None;
    Ok(ReadyReadIrqPair { mmc : mmc_receipt, dma : dma_receipt })
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
        mmc.read_interrupts = 0;
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
    mmc.read_interrupts = 0;
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
    read_interrupts : u32,
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
               read_interrupts : 0,
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
        self.read_interrupts = 0;
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
                self.read_interrupts = 0;
                Ok(())
            },
        }
    }
    pub fn take_read_receipt(&mut self) -> Option<MmcReadIrqReceipt> {
        self.read_pending.take()
    }

    fn record_read_interrupts(&mut self,
                              transaction : ReadTransactionId,
                              interrupts : u32) -> bool {
        self.read_interrupts |= interrupts;
        if !read_interrupt_snapshot_terminal(self.read_interrupts) { return false; }
        self.read_pending = Some(MmcReadIrqReceipt {
            transaction,
            interrupts : self.read_interrupts,
        });
        self.read_armed = None;
        self.read_interrupts = 0;
        true
    }
    pub fn registers(&self) -> &R { &self.registers }
    pub fn registers_mut(&mut self) -> &mut R { &mut self.registers }
    pub fn into_registers(self) -> R { self.registers }
}

impl<R : RegisterIo> MmcCommandOwner<R> {
    /// Poll and clear another MMC snapshot while the source remains masked.
    /// No LIOINTC acknowledgement or rearm evidence is created.
    pub fn recheck_masked_read(&mut self) -> Result<bool, MmcReadRecheckError> {
        if self.read_pending.is_some() {
            return Err(MmcReadRecheckError::Owner(ReadIrqOwnerError::PendingNotConsumed));
        }
        let transaction = self.read_armed.ok_or(
            MmcReadRecheckError::Owner(ReadIrqOwnerError::NotArmed))?;
        let interrupts = match clear_masked_interrupt_snapshot(&mut self.registers) {
            Ok(interrupts) => interrupts,
            Err(error) => {
                self.last_error = Some(error);
                return Err(MmcReadRecheckError::Ack(error));
            },
        };
        self.last_error = None;
        self.last_read_error = None;
        Ok(self.record_read_interrupts(transaction, interrupts))
    }
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
                } else if let Some(transaction) = self.read_armed {
                    self.record_read_interrupts(transaction, receipt.interrupts);
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
        fail_read : bool,
    }

    impl RegisterIo for MockRegisters {
        fn read32(&mut self, offset : usize) -> Result<u32, MmcError> {
            if self.fail_read { return Err(MmcError::RegisterOutOfRange); }
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
            irq, MockRegisters { status : COMMAND_SENT, ..MockRegisters::default() }));
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
            MockRegisters { status : COMMAND_SENT | 1, ..MockRegisters::default() });
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
            MockRegisters { status : COMMAND_SENT | 1, ..MockRegisters::default() });
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
    fn mmc_read_owner_rechecks_masked_split_completion_before_receipt() {
        let current = transaction(43);
        let mmc_irq = GlobalIrq::from_bank_local(0, 31).unwrap();
        let mut mmc = MmcCommandOwner::new(
            mmc_irq,
            MockRegisters { status : COMMAND_SENT, ..MockRegisters::default() });
        mmc.arm_read(current).unwrap();
        assert_eq!(mmc.handle(acknowledged(mmc_irq)), IrqDisposition::KeepMasked);
        assert_eq!(mmc.read_binding(), Some(ReadIrqOwnerBinding::Armed(current)));
        assert_eq!(mmc.take_read_receipt(), None);
        assert_eq!(mmc.registers().writes, [(REG_INT, COMMAND_SENT)]);

        mmc.registers_mut().status = 1;
        assert_eq!(mmc.recheck_masked_read(), Ok(true));
        assert_eq!(mmc.read_binding(), Some(ReadIrqOwnerBinding::Pending(current)));
        let receipt = mmc.take_read_receipt().unwrap();
        assert_eq!(receipt.transaction, current);
        assert_eq!(receipt.interrupts, COMMAND_SENT | 1);
        assert_eq!(mmc.registers().writes,
                   [(REG_INT, COMMAND_SENT), (REG_INT, 1)]);
        assert_eq!(mmc.recheck_masked_read(),
                   Err(MmcReadRecheckError::Owner(ReadIrqOwnerError::NotArmed)));

        let aborted = transaction(44);
        let replacement = transaction(45);
        mmc.registers_mut().status = COMMAND_SENT;
        mmc.arm_read(aborted).unwrap();
        assert_eq!(mmc.handle(acknowledged(mmc_irq)), IrqDisposition::KeepMasked);
        mmc.disarm_read(aborted).unwrap();
        mmc.registers_mut().status = 1;
        mmc.arm_read(replacement).unwrap();
        assert_eq!(mmc.handle(acknowledged(mmc_irq)), IrqDisposition::KeepMasked);
        assert_eq!(mmc.read_binding(), Some(ReadIrqOwnerBinding::Armed(replacement)));
        assert_eq!(mmc.take_read_receipt(), None);
        mmc.disarm_read(replacement).unwrap();
    }

    #[test]
    fn bounded_runtime_recheck_covers_terminal_timeout_fault_and_wrong_generation() {
        let mmc_irq = GlobalIrq::from_bank_local(0, 31).unwrap();
        let dma_irq = GlobalIrq::from_bank_local(1, 13).unwrap();
        let current = transaction(46);
        let timeout = transaction(47);
        let wrong = transaction(48);
        let mut runtime = owner_runtime(mmc_irq, dma_irq, false);
        let mmc = runtime.owner_mut(mmc_irq).unwrap();
        let BoardIrqOwner::MmcCommand(mmc) = mmc else { panic!("wrong MMC variant") };
        mmc.arm_read(current).unwrap();

        let recheck = BoundedMmcReadRecheck::new(current, 3).unwrap();
        let recheck = match recheck.step(&mut runtime, mmc_irq).unwrap() {
            BoundedMmcReadRecheckProgress::Pending(recheck) => recheck,
            _ => panic!("empty first sample was not pending"),
        };
        assert_eq!(recheck.remaining(), 2);
        assert_eq!(recheck.polls_completed(), 1);
        let mmc = runtime.owner_mut(mmc_irq).unwrap();
        let BoardIrqOwner::MmcCommand(mmc) = mmc else { panic!("wrong MMC variant") };
        mmc.registers_mut().status = COMMAND_SENT;
        let recheck = match recheck.step(&mut runtime, mmc_irq).unwrap() {
            BoundedMmcReadRecheckProgress::Pending(recheck) => recheck,
            _ => panic!("CSENT-only sample was not pending"),
        };
        let mmc = runtime.owner_mut(mmc_irq).unwrap();
        let BoardIrqOwner::MmcCommand(mmc) = mmc else { panic!("wrong MMC variant") };
        mmc.registers_mut().status = 1;
        assert_eq!(recheck.step(&mut runtime, mmc_irq).unwrap(),
                   BoundedMmcReadRecheckProgress::Terminal {
                       transaction : current, polls_completed : 3,
                   });
        let mmc = runtime.owner_mut(mmc_irq).unwrap();
        let BoardIrqOwner::MmcCommand(mmc) = mmc else { panic!("wrong MMC variant") };
        assert_eq!(mmc.take_read_receipt().unwrap().interrupts, COMMAND_SENT | 1);
        mmc.registers_mut().status = 0;
        mmc.arm_read(timeout).unwrap();

        let failure = BoundedMmcReadRecheck::new(wrong, 1).unwrap()
            .step(&mut runtime, mmc_irq).unwrap_err();
        assert_eq!(failure.error,
                   BoundedMmcReadRecheckError::Binding(
                       Some(ReadIrqOwnerBinding::Armed(timeout))));
        assert_eq!(failure.recheck.transaction(), wrong);
        let recheck = BoundedMmcReadRecheck::new(timeout, 2).unwrap();
        let recheck = match recheck.step(&mut runtime, mmc_irq).unwrap() {
            BoundedMmcReadRecheckProgress::Pending(recheck) => recheck,
            _ => panic!("first empty timeout sample was not pending"),
        };
        assert_eq!(recheck.step(&mut runtime, mmc_irq).unwrap(),
                   BoundedMmcReadRecheckProgress::Timeout {
                       transaction : timeout, polls_completed : 2,
                   });
        let mmc = runtime.owner_mut(mmc_irq).unwrap();
        let BoardIrqOwner::MmcCommand(mmc) = mmc else { panic!("wrong MMC variant") };
        assert_eq!(mmc.read_binding(), Some(ReadIrqOwnerBinding::Armed(timeout)));
        mmc.registers_mut().status = 1 << 20;
        let failure = BoundedMmcReadRecheck::new(timeout, 1).unwrap()
            .step(&mut runtime, mmc_irq).unwrap_err();
        assert_eq!(failure.error,
                   BoundedMmcReadRecheckError::Recheck(MmcReadRecheckError::Ack(
                       MmcIrqAckError::UnknownPending(1 << 20))));
        assert_eq!(failure.recovery_cause(),
                   Some(ReadRecoveryCause::RecheckFault {
                       error : MmcReadRecheckError::Ack(
                           MmcIrqAckError::UnknownPending(1 << 20)),
                       polls_completed : 0,
                       remaining : 1,
                   }));
        let mmc = runtime.owner_mut(mmc_irq).unwrap();
        let BoardIrqOwner::MmcCommand(mmc) = mmc else { panic!("wrong MMC variant") };
        assert_eq!(mmc.read_binding(), Some(ReadIrqOwnerBinding::Armed(timeout)));
        mmc.registers_mut().fail_read = true;
        let failure = BoundedMmcReadRecheck::new(timeout, 3).unwrap()
            .step(&mut runtime, mmc_irq).unwrap_err();
        let io_error = MmcReadRecheckError::Ack(MmcIrqAckError::Io(
            MmcError::RegisterOutOfRange));
        assert_eq!(failure.error, BoundedMmcReadRecheckError::Recheck(io_error));
        assert_eq!(failure.recovery_cause(),
                   Some(ReadRecoveryCause::RecheckFault {
                       error : io_error, polls_completed : 0, remaining : 3,
                   }));
        let mmc = runtime.owner_mut(mmc_irq).unwrap();
        let BoardIrqOwner::MmcCommand(mmc) = mmc else { panic!("wrong MMC variant") };
        mmc.registers_mut().fail_read = false;
        mmc.disarm_read(timeout).unwrap();
        assert_eq!(BoundedMmcReadRecheck::new(timeout, 0),
                   Err(BoundedMmcReadRecheckError::InvalidBudget));
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
            MockRegisters { status : COMMAND_SENT | 1, ..MockRegisters::default() });
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
        let failure = retire_read_irq_owners(&mut runtime, dma_irq, mmc_irq, armed)
            .expect_err("reversed owner variants retired");
        assert_eq!(failure.error, ReadIrqRetireError::MmcOwnerVariant);
        let armed = failure.into_armed();
        assert_eq!(armed.transaction(), second);
        let drained = retire_read_irq_owners(&mut runtime, mmc_irq, dma_irq, armed)
            .unwrap_or_else(|_| panic!("committed generation did not drain"));
        assert!(drained.mmc.is_none());
        assert!(drained.dma.is_none());
    }

    #[test]
    fn recovery_report_rejects_wrong_generation_without_draining_owners() {
        let mmc_irq = GlobalIrq::from_bank_local(0, 31).unwrap();
        let dma_irq = GlobalIrq::from_bank_local(1, 13).unwrap();
        let current = transaction(83);
        let wrong = transaction(84);
        let mut runtime = owner_runtime(mmc_irq, dma_irq, false);
        let armed = reserve_read_irq_owners(&mut runtime, mmc_irq, dma_irq, current)
            .unwrap_or_else(|_| panic!("guard reservation failed"))
            .commit();
        let cause = ReadRecoveryCause::Timeout { polls_completed : 4 };
        let failure = retire_quiesced_read_recovery(
            &mut runtime,
            mmc_irq,
            dma_irq,
            QuiescedReadIrqs::fixture(wrong),
            cause)
            .expect_err("wrong recovery generation drained current owners");
        assert_eq!(failure.error,
                   ReadIrqRetireError::Drain(ReadPairOwnerFailure {
                       owner : ReadPairOwner::Mmc,
                       error : ReadIrqOwnerError::WrongTransaction,
                   }));
        assert_eq!(failure.cause, cause);
        assert_eq!(failure.into_quiesced().transaction(), wrong);
        let BoardIrqOwner::MmcCommand(mmc) = runtime.owner(mmc_irq).unwrap() else {
            panic!("wrong MMC variant")
        };
        assert_eq!(mmc.read_binding(), Some(ReadIrqOwnerBinding::Armed(current)));
        let BoardIrqOwner::ApbDmaDeferred(dma) = runtime.owner(dma_irq).unwrap() else {
            panic!("wrong DMA variant")
        };
        assert_eq!(dma.read_binding(), Some(ReadIrqOwnerBinding::Armed(current)));
        retire_read_irq_owners(&mut runtime, mmc_irq, dma_irq, armed)
            .unwrap_or_else(|_| panic!("current generation did not remain drainable"));
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

    #[test]
    fn runtime_pair_take_waits_for_both_pending_and_preserves_wrong_generation() {
        let mmc_irq = GlobalIrq::from_bank_local(0, 31).unwrap();
        let dma_irq = GlobalIrq::from_bank_local(1, 13).unwrap();
        let current = transaction(111);
        let wrong = transaction(112);
        let mut runtime = owner_runtime(mmc_irq, dma_irq, false);
        let armed = reserve_read_irq_owners(&mut runtime, mmc_irq, dma_irq, current)
            .unwrap_or_else(|_| panic!("guard reservation failed"))
            .commit();

        let dma = runtime.owner_mut(dma_irq).unwrap();
        assert_eq!(dma.handle(acknowledged(dma_irq)), IrqDisposition::KeepMasked);
        let failure = take_pending_read_irq_pair(&mut runtime, mmc_irq, dma_irq, armed)
            .expect_err("DMA-only pending pair was consumed");
        assert_eq!(failure.error,
                   ReadPendingPairError::MmcBinding(
                       Some(ReadIrqOwnerBinding::Armed(current))));
        let armed = failure.into_armed();
        let BoardIrqOwner::ApbDmaDeferred(dma) = runtime.owner(dma_irq).unwrap() else {
            panic!("wrong DMA variant")
        };
        assert_eq!(dma.read_binding(), Some(ReadIrqOwnerBinding::Pending(current)));

        let mmc = runtime.owner_mut(mmc_irq).unwrap();
        let BoardIrqOwner::MmcCommand(mmc) = mmc else { panic!("wrong MMC variant") };
        mmc.registers.status = COMMAND_SENT | 1;
        assert_eq!(mmc.handle(acknowledged(mmc_irq)), IrqDisposition::KeepMasked);

        let wrong_armed = ArmedReadIrqs::fixture(wrong);
        let failure = take_pending_read_irq_pair(&mut runtime,
                                                 mmc_irq,
                                                 dma_irq,
                                                 wrong_armed)
            .expect_err("wrong generation consumed pending receipts");
        assert_eq!(failure.error,
                   ReadPendingPairError::MmcBinding(
                       Some(ReadIrqOwnerBinding::Pending(current))));
        assert_eq!(failure.into_armed().transaction(), wrong);

        let pair = take_pending_read_irq_pair(&mut runtime, mmc_irq, dma_irq, armed)
            .unwrap_or_else(|_| panic!("matching pending pair rejected"));
        assert_eq!(pair.transaction(), current);
        assert_eq!(pair.mmc.interrupts, COMMAND_SENT | 1);
        assert_eq!(pair.dma.acknowledged.irq(), dma_irq);
        let BoardIrqOwner::MmcCommand(mmc) = runtime.owner(mmc_irq).unwrap() else {
            panic!("wrong MMC variant")
        };
        assert_eq!(mmc.read_binding(), None);
        let BoardIrqOwner::ApbDmaDeferred(dma) = runtime.owner(dma_irq).unwrap() else {
            panic!("wrong DMA variant")
        };
        assert_eq!(dma.read_binding(), None);
    }
}

//! Production storage for one deferred MMC/APBDMA read coordinator.
//!
//! The slot never publishes an MMC command, starts DMA or rearms an interrupt
//! source. An explicit recheck service performs at most one masked MMC status
//! sample through the caller-provided runtime.
//! `UNVERIFIED_ON_HARDWARE`: worker scheduling and late-IRQ timing still need
//! validation on a physical 2K1000LA board before this slot can gate rearm.

use crate::{board_irq_owner::{BoardIrqOwner, BoundedMmcReadRecheck,
                              BoundedMmcReadRecheckError,
                              BoundedMmcReadRecheckStep, ArmedReadIrqs,
                              QuiescedReadIrqs, ReadyReadIrqPair,
                              ClaimedReadCompletionEvidence,
                              ClaimedReadRecoveryEvidence, DrainedReadIrqs,
                              ReadPendingPairError,
                              ReadRecoveryCause, ReadRecoveryReport,
                              ReadIrqRetireError, ReadTransactionId,
                              retire_quiesced_read_recovery,
                              take_pending_read_irq_pair},
            diagnostic_slot::{DiagnosticRuntimeSlot, DiagnosticSlotState,
                              DrainError, RuntimeReservation, RuntimeService, SlotError},
            apbdma::{self, Direction as ApbDmaDirection, PreparedSession, TransferPlan},
            irq_domain::GlobalIrq,
            mmc::{ReadBlockRequest, RegisterIo}};
use api_v0::{dma::{DmaCoherency, DmaDirection, DmaMapping, DmaRegion}, DriverError};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadCoordinatorPhase {
    Reserved,
    Published,
    Rechecking,
    Terminal,
    CompletionClaimed,
    CompletionFinalized,
    RecoveryPending,
    RecoveryRecorded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadCoordinatorSnapshot {
    pub transaction : ReadTransactionId,
    pub phase : ReadCoordinatorPhase,
    pub poll_budget : Option<u16>,
    pub remaining : Option<u16>,
    pub polls_completed : Option<u16>,
    pub recovery_cause : Option<ReadRecoveryCause>,
    pub partial_mmc_interrupts : Option<u32>,
    pub has_mmc_receipt : bool,
    pub has_dma_receipt : bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadCoordinatorError {
    Slot(SlotError),
    WrongTransaction {
        expected : ReadTransactionId,
        actual : ReadTransactionId,
    },
    WrongPhase {
        expected : ReadCoordinatorPhase,
        actual : ReadCoordinatorPhase,
    },
    InvalidPollBudget,
    InvalidPollProgress,
    RecoveryCauseMismatch {
        expected : ReadRecoveryCause,
        actual : ReadRecoveryCause,
    },
    InvalidCompletionEvidence(crate::mmc::ReadCompletionEvidence),
    CompletionMustBeFinalized,
    RecoveryMustBeRecorded,
    RecoveryMustBeTaken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadRequestBufferError {
    WrongLength { expected : usize, actual : usize },
    WrongDirection(DmaDirection),
    Dma(DriverError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadRequestDmaLeaseError {
    InvalidPlan,
    NotDeviceToMemory,
    DescriptorAddressMismatch,
    PayloadAddressMismatch,
    PayloadLengthMismatch,
    Dma(DriverError),
}

impl From<DriverError> for ReadRequestDmaLeaseError {
    fn from(error : DriverError) -> Self { Self::Dma(error) }
}

impl From<DriverError> for ReadRequestBufferError {
    fn from(error : DriverError) -> Self { Self::Dma(error) }
}

#[must_use = "retry recording or retain the linear recovery report"]
pub struct RecordRecoveryFailure {
    pub error : ReadCoordinatorError,
    pub report : ReadRecoveryReport,
}

#[must_use = "retry storing the bounded recheck token or recover it"]
pub struct RecordRecheckFailure {
    pub error : ReadCoordinatorError,
    pub recheck : BoundedMmcReadRecheck,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadCoordinatorStepProgress {
    Pending {
        transaction : ReadTransactionId,
        remaining : u16,
        polls_completed : u16,
    },
    Terminal {
        transaction : ReadTransactionId,
        polls_completed : u16,
    },
    RecoveryPending {
        transaction : ReadTransactionId,
        cause : ReadRecoveryCause,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadCoordinatorStepFailure {
    pub error : BoundedMmcReadRecheckError,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadCoordinatorRecoveryError {
    WrongTransaction {
        expected : ReadTransactionId,
        actual : ReadTransactionId,
    },
    Retire(ReadIrqRetireError),
}

#[must_use = "recover the quiesced token and retry the recovery service"]
pub struct ReadCoordinatorRecoveryFailure {
    pub error : ReadCoordinatorRecoveryError,
    pub cause : ReadRecoveryCause,
    quiesced : QuiescedReadIrqs,
}

#[must_use = "recover the claimed evidence and retry the recovery service"]
pub struct ReadClaimedRecoveryArchiveFailure {
    pub error : ReadCoordinatorError,
    pub evidence : ClaimedReadRecoveryEvidence,
}

#[must_use = "recover the completion evidence and retry finalization"]
pub struct ReadCompletionFinalizeFailure {
    pub error : ReadCoordinatorError,
    pub evidence : ClaimedReadCompletionEvidence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadTerminalClaimError {
    WrongTransaction {
        expected : ReadTransactionId,
        actual : ReadTransactionId,
    },
    Pair(ReadPendingPairError),
}

#[must_use = "recover the armed token and retry the terminal service"]
pub struct ReadTerminalClaimFailure {
    pub error : ReadTerminalClaimError,
    armed : ArmedReadIrqs,
}

impl ReadTerminalClaimFailure {
    pub fn into_armed(self) -> ArmedReadIrqs { self.armed }
}

impl ReadCoordinatorRecoveryFailure {
    pub fn into_quiesced(self) -> QuiescedReadIrqs { self.quiesced }
}

enum ReadCoordinatorState {
    Reserved {
        transaction : ReadTransactionId,
    },
    Published {
        transaction : ReadTransactionId,
        poll_budget : u16,
    },
    Rechecking {
        transaction : ReadTransactionId,
        poll_budget : u16,
        recheck : BoundedMmcReadRecheck,
    },
    Terminal {
        transaction : ReadTransactionId,
        polls_completed : u16,
    },
    #[cfg_attr(not(test), allow(dead_code))]
    CompletionClaimed {
        transaction : ReadTransactionId,
    },
    CompletionFinalized {
        transaction : ReadTransactionId,
    },
    RecoveryPending {
        transaction : ReadTransactionId,
        cause : ReadRecoveryCause,
    },
    RecoveryRecorded(ReadRecoveryReport),
}

impl ReadCoordinatorState {
    fn transaction(&self) -> ReadTransactionId {
        match self {
            Self::Reserved { transaction } |
            Self::Published { transaction, .. } |
            Self::Rechecking { transaction, .. } |
            Self::Terminal { transaction, .. } |
            Self::CompletionClaimed { transaction } |
            Self::CompletionFinalized { transaction } |
            Self::RecoveryPending { transaction, .. } => *transaction,
            Self::RecoveryRecorded(report) => report.transaction,
        }
    }

    fn phase(&self) -> ReadCoordinatorPhase {
        match self {
            Self::Reserved { .. } => ReadCoordinatorPhase::Reserved,
            Self::Published { .. } => ReadCoordinatorPhase::Published,
            Self::Rechecking { .. } => ReadCoordinatorPhase::Rechecking,
            Self::Terminal { .. } => ReadCoordinatorPhase::Terminal,
            Self::CompletionClaimed { .. } => ReadCoordinatorPhase::CompletionClaimed,
            Self::CompletionFinalized { .. } => ReadCoordinatorPhase::CompletionFinalized,
            Self::RecoveryPending { .. } => ReadCoordinatorPhase::RecoveryPending,
            Self::RecoveryRecorded(_) => ReadCoordinatorPhase::RecoveryRecorded,
        }
    }

    fn snapshot(&self) -> ReadCoordinatorSnapshot {
        let mut snapshot = ReadCoordinatorSnapshot {
            transaction : self.transaction(),
            phase : self.phase(),
            poll_budget : None,
            remaining : None,
            polls_completed : None,
            recovery_cause : None,
            partial_mmc_interrupts : None,
            has_mmc_receipt : false,
            has_dma_receipt : false,
        };
        match self {
            Self::Published { poll_budget, .. } => snapshot.poll_budget = Some(*poll_budget),
            Self::Rechecking { poll_budget, recheck, .. } => {
                snapshot.poll_budget = Some(*poll_budget);
                snapshot.remaining = Some(recheck.remaining());
                snapshot.polls_completed = Some(recheck.polls_completed());
            },
            Self::Terminal { polls_completed, .. } =>
                snapshot.polls_completed = Some(*polls_completed),
            Self::CompletionClaimed { .. } => {},
            Self::CompletionFinalized { .. } => {},
            Self::RecoveryPending { cause, .. } => snapshot.recovery_cause = Some(*cause),
            Self::RecoveryRecorded(report) => {
                snapshot.recovery_cause = Some(report.cause);
                snapshot.partial_mmc_interrupts = Some(report.partial_mmc_interrupts);
                snapshot.has_mmc_receipt = report.drained.mmc.is_some() ||
                                           report.claimed.is_some();
                snapshot.has_dma_receipt = report.drained.dma.is_some() ||
                                           report.claimed.is_some();
            },
            Self::Reserved { .. } => {},
        }
        snapshot
    }
}

pub struct ReadCoordinatorSlot {
    inner : DiagnosticRuntimeSlot<ReadCoordinatorState>,
}

/// Production-owned identity and geometry for one deferred read.
///
/// The request is intentionally separate from the hardware session: callers
/// may retain it while a worker waits for MMC/DMA progress, while the slot
/// remains the sole owner of lifecycle transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReadRequest {
    transaction : ReadTransactionId,
    block : ReadBlockRequest,
}

impl ReadRequest {
    pub const fn new(transaction : ReadTransactionId,
                     block : ReadBlockRequest)
                     -> Self { Self { transaction, block } }

    pub const fn transaction(&self) -> ReadTransactionId { self.transaction }

    pub const fn block(&self) -> ReadBlockRequest { self.block }

    pub fn bind_dma_buffer<C : DmaCoherency>(
        &self,
        mapping : DmaMapping<C>)
        -> Result<ReadRequestBuffer<C>, ReadRequestBufferError> {
        let region = mapping.identity_region();
        if region.length() != self.block.byte_length {
            return Err(ReadRequestBufferError::WrongLength {
                expected : self.block.byte_length,
                actual : region.length(),
            });
        }
        if mapping.identity_direction() != DmaDirection::FromDevice {
            return Err(ReadRequestBufferError::WrongDirection(mapping.identity_direction()));
        }
        Ok(ReadRequestBuffer { request : *self, mapping })
    }

    pub fn reserve<'a>(&self,
                       slot : &'a ReadCoordinatorSlot)
                       -> Result<ReadRequestReservation<'a>, ReadCoordinatorError> {
        slot.reserve(self.transaction).map(|reservation| ReadRequestReservation {
            reservation,
            slot,
            request : *self,
        })
    }
}

/// A production-owned read payload mapping. The mapping remains CPU-owned
/// until the caller explicitly prepares it, and cannot be released while the
/// device owns it. The actual cache/barrier behavior remains
/// `UNVERIFIED_ON_HARDWARE` for LS2K1000LA.
pub struct ReadRequestBuffer<C> {
    request : ReadRequest,
    mapping : DmaMapping<C>,
}

impl<C : DmaCoherency> ReadRequestBuffer<C> {
    pub const fn request(&self) -> ReadRequest { self.request }

    pub fn region(&self) -> Result<DmaRegion, ReadRequestBufferError> {
        self.mapping.cpu_region().map_err(Into::into)
    }

    pub fn prepare_for_device(&mut self) -> Result<DmaRegion, ReadRequestBufferError> {
        self.mapping.prepare_for_device().map_err(Into::into)
    }

    pub fn complete_from_device(&mut self) -> Result<DmaRegion, ReadRequestBufferError> {
        self.mapping.complete_from_device().map_err(Into::into)
    }

    pub const fn is_cpu_owned(&self) -> bool { self.mapping.is_cpu_owned() }

}

/// Paired descriptor/payload mappings owned by one production read request.
/// The plan and both mapping identities are checked before either mapping is
/// handed to the APBDMA typestate machine.
pub struct ReadRequestDmaLease<D, P> {
    request : ReadRequest,
    plan : TransferPlan,
    descriptor : DmaMapping<D>,
    payload : DmaMapping<P>,
}

impl<D : DmaCoherency, P : DmaCoherency> ReadRequestDmaLease<D, P> {
    pub fn bind(request : ReadRequest,
               plan : TransferPlan,
               descriptor : DmaMapping<D>,
               payload : DmaMapping<P>)
               -> Result<Self, ReadRequestDmaLeaseError> {
        let descriptor_region = descriptor.identity_region();
        let payload_region = payload.identity_region();
        if plan.direction != ApbDmaDirection::DeviceToMemory {
            return Err(ReadRequestDmaLeaseError::NotDeviceToMemory);
        }
        if plan.byte_length != request.block.byte_length ||
           !plan.invalidate_buffer_after || !plan.clean_descriptor_before {
            return Err(ReadRequestDmaLeaseError::InvalidPlan);
        }
        if plan.descriptor_physical_address != descriptor_region.physical_address() {
            return Err(ReadRequestDmaLeaseError::DescriptorAddressMismatch);
        }
        if plan.memory_physical_address != payload_region.physical_address() {
            return Err(ReadRequestDmaLeaseError::PayloadAddressMismatch);
        }
        if payload_region.length() != request.block.byte_length ||
           payload.identity_direction() != DmaDirection::FromDevice ||
           descriptor.identity_direction() != DmaDirection::Bidirectional {
            return Err(ReadRequestDmaLeaseError::PayloadLengthMismatch);
        }
        Ok(Self { request, plan, descriptor, payload })
    }

    pub const fn request(&self) -> ReadRequest { self.request }

    pub const fn plan(&self) -> TransferPlan { self.plan }

    pub fn prepare_session(&mut self)
                           -> Result<PreparedSession<'_, D, P>, ReadRequestDmaLeaseError> {
        apbdma::prepare_session(self.plan, &mut self.descriptor, &mut self.payload)
            .map_err(Into::into)
    }
}

impl ReadCoordinatorSlot {
    pub const fn new() -> Self { Self { inner : DiagnosticRuntimeSlot::new() } }

    pub fn state(&self) -> DiagnosticSlotState { self.inner.state() }

    pub fn reserve(&self,
                   transaction : ReadTransactionId)
                   -> Result<ReadCoordinatorReservation<'_>, ReadCoordinatorError> {
        self.inner.reserve()
            .map(|reservation| ReadCoordinatorReservation { reservation, transaction })
            .map_err(ReadCoordinatorError::Slot)
    }

    pub fn snapshot(&self) -> Result<ReadCoordinatorSnapshot, ReadCoordinatorError> {
        self.inner.with_live_mut(|state| state.snapshot())
                  .map_err(ReadCoordinatorError::Slot)
    }

    pub fn mark_published(&self,
                          transaction : ReadTransactionId,
                          poll_budget : u16)
                          -> Result<(), ReadCoordinatorError> {
        if poll_budget == 0 { return Err(ReadCoordinatorError::InvalidPollBudget); }
        self.with_state(transaction, |state| {
            if state.phase() != ReadCoordinatorPhase::Reserved {
                return Err(ReadCoordinatorError::WrongPhase {
                    expected : ReadCoordinatorPhase::Reserved,
                    actual : state.phase(),
                });
            }
            *state = ReadCoordinatorState::Published { transaction, poll_budget };
            Ok(())
        })
    }

    pub fn record_recheck(&self,
                          recheck : BoundedMmcReadRecheck)
                          -> Result<(), RecordRecheckFailure> {
        let transaction = recheck.transaction();
        let mut recheck = Some(recheck);
        let result = self.with_state(transaction, |state| {
            let poll_budget = match state {
                ReadCoordinatorState::Published { poll_budget, .. } => *poll_budget,
                _ => {
                    return Err(ReadCoordinatorError::WrongPhase {
                        expected : ReadCoordinatorPhase::Published,
                        actual : state.phase(),
                    });
                },
            };
            let value = recheck.as_ref().unwrap();
            if value.remaining().checked_add(value.polls_completed()) != Some(poll_budget) {
                return Err(ReadCoordinatorError::InvalidPollProgress);
            }
            *state = ReadCoordinatorState::Rechecking {
                transaction,
                poll_budget,
                recheck : recheck.take().unwrap(),
            };
            Ok(())
        });
        result.map_err(|error| RecordRecheckFailure {
            error, recheck : recheck.unwrap(),
        })
    }

    pub fn service_recheck(&self,
                           transaction : ReadTransactionId)
                           -> Result<ReadRecheckService<'_>, ReadCoordinatorError> {
        let service = self.inner.service().map_err(ReadCoordinatorError::Slot)?;
        if service.transaction() != transaction {
            return Err(ReadCoordinatorError::WrongTransaction {
                expected : service.transaction(), actual : transaction,
            });
        }
        if service.phase() != ReadCoordinatorPhase::Rechecking {
            return Err(ReadCoordinatorError::WrongPhase {
                expected : ReadCoordinatorPhase::Rechecking,
                actual : service.phase(),
            });
        }
        Ok(ReadRecheckService { service, transaction })
    }

    pub fn service_recovery(&self,
                            transaction : ReadTransactionId)
                            -> Result<ReadRecoveryService<'_>, ReadCoordinatorError> {
        let service = self.inner.service().map_err(ReadCoordinatorError::Slot)?;
        if service.transaction() != transaction {
            return Err(ReadCoordinatorError::WrongTransaction {
                expected : service.transaction(), actual : transaction,
            });
        }
        if service.phase() != ReadCoordinatorPhase::RecoveryPending {
            return Err(ReadCoordinatorError::WrongPhase {
                expected : ReadCoordinatorPhase::RecoveryPending,
                actual : service.phase(),
            });
        }
        Ok(ReadRecoveryService { service, transaction })
    }

    pub fn service_terminal(&self,
                            transaction : ReadTransactionId)
                            -> Result<ReadTerminalService<'_>, ReadCoordinatorError> {
        let service = self.inner.service().map_err(ReadCoordinatorError::Slot)?;
        if service.transaction() != transaction {
            return Err(ReadCoordinatorError::WrongTransaction {
                expected : service.transaction(), actual : transaction,
            });
        }
        if service.phase() != ReadCoordinatorPhase::Terminal {
            return Err(ReadCoordinatorError::WrongPhase {
                expected : ReadCoordinatorPhase::Terminal,
                actual : service.phase(),
            });
        }
        Ok(ReadTerminalService { service, transaction })
    }

    pub fn service_claimed_completion(
        &self,
        transaction : ReadTransactionId)
        -> Result<ReadClaimedCompletionService<'_>, ReadCoordinatorError> {
        let service = self.inner.service().map_err(ReadCoordinatorError::Slot)?;
        if service.transaction() != transaction {
            return Err(ReadCoordinatorError::WrongTransaction {
                expected : service.transaction(), actual : transaction,
            });
        }
        if service.phase() != ReadCoordinatorPhase::CompletionClaimed {
            return Err(ReadCoordinatorError::WrongPhase {
                expected : ReadCoordinatorPhase::CompletionClaimed,
                actual : service.phase(),
            });
        }
        Ok(ReadClaimedCompletionService { service, transaction })
    }

    pub fn record_recovery(&self,
                           report : ReadRecoveryReport)
                           -> Result<(), RecordRecoveryFailure> {
        let transaction = report.transaction;
        let mut report = Some(report);
        let result = self.inner.with_live_mut(|state| {
            if state.transaction() != transaction {
                return Err(ReadCoordinatorError::WrongTransaction {
                    expected : state.transaction(), actual : transaction,
                });
            }
            if !matches!(state,
                         ReadCoordinatorState::Published { .. } |
                         ReadCoordinatorState::Rechecking { .. } |
                         ReadCoordinatorState::RecoveryPending { .. })
            {
                return Err(ReadCoordinatorError::WrongPhase {
                    expected : ReadCoordinatorPhase::Rechecking,
                    actual : state.phase(),
                });
            }
            if let ReadCoordinatorState::RecoveryPending { cause, .. } = state {
                if *cause != report.as_ref().unwrap().cause {
                    return Err(ReadCoordinatorError::RecoveryCauseMismatch {
                        expected : *cause,
                        actual : report.as_ref().unwrap().cause,
                    });
                }
            }
            *state = ReadCoordinatorState::RecoveryRecorded(report.take().unwrap());
            Ok(())
        });
        match result {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => Err(RecordRecoveryFailure {
                error, report : report.unwrap(),
            }),
            Err(error) => Err(RecordRecoveryFailure {
                error : ReadCoordinatorError::Slot(error), report : report.unwrap(),
            }),
        }
    }

    pub fn take_recovery(&self,
                         transaction : ReadTransactionId)
                         -> Result<ReadRecoveryReport, ReadCoordinatorError> {
        let mut report = None;
        let result = self.inner.drain(|state| {
            self.validate_transaction(state, transaction)?;
            if !matches!(state, ReadCoordinatorState::RecoveryRecorded(_)) {
                return Err(ReadCoordinatorError::WrongPhase {
                    expected : ReadCoordinatorPhase::RecoveryRecorded,
                    actual : state.phase(),
                });
            }
            let placeholder = ReadCoordinatorState::Reserved { transaction };
            let ReadCoordinatorState::RecoveryRecorded(value) =
                core::mem::replace(state, placeholder) else { unreachable!() };
            report = Some(value);
            Ok(())
        });
        match result {
            Ok(()) => Ok(report.unwrap()),
            Err(DrainError::Slot(error)) => Err(ReadCoordinatorError::Slot(error)),
            Err(DrainError::Operation(error)) => Err(error),
        }
    }

    /// Release a cancelled or successfully completed coordinator. Recorded
    /// recovery evidence must instead be removed with `take_recovery`.
    pub fn release(&self,
                   transaction : ReadTransactionId)
                   -> Result<(), ReadCoordinatorError> {
        match self.inner.drain(|state| {
            self.validate_transaction(state, transaction)?;
            if state.phase() == ReadCoordinatorPhase::RecoveryRecorded {
                return Err(ReadCoordinatorError::RecoveryMustBeTaken);
            }
            if state.phase() == ReadCoordinatorPhase::RecoveryPending {
                return Err(ReadCoordinatorError::RecoveryMustBeRecorded);
            }
            if state.phase() == ReadCoordinatorPhase::CompletionClaimed {
                return Err(ReadCoordinatorError::CompletionMustBeFinalized);
            }
            Ok(())
        }) {
            Ok(()) => Ok(()),
            Err(DrainError::Slot(error)) => Err(ReadCoordinatorError::Slot(error)),
            Err(DrainError::Operation(error)) => Err(error),
        }
    }

    fn with_state(&self,
                  transaction : ReadTransactionId,
                  f : impl FnOnce(&mut ReadCoordinatorState)
                      -> Result<(), ReadCoordinatorError>)
                  -> Result<(), ReadCoordinatorError> {
        self.inner.with_live_mut(|state| {
            self.validate_transaction(state, transaction)?;
            f(state)
        })
        .map_err(ReadCoordinatorError::Slot)?
    }

    fn validate_transaction(&self,
                            state : &ReadCoordinatorState,
                            transaction : ReadTransactionId)
                            -> Result<(), ReadCoordinatorError> {
        let expected = state.transaction();
        if expected == transaction {
            Ok(())
        } else {
            Err(ReadCoordinatorError::WrongTransaction {
                expected, actual : transaction,
            })
        }
    }
}

#[must_use = "execute one recheck step or drop the service to restore LIVE"]
pub struct ReadRecheckService<'a> {
    service : RuntimeService<'a, ReadCoordinatorState>,
    transaction : ReadTransactionId,
}

impl ReadRecheckService<'_> {
    pub const fn transaction(&self) -> ReadTransactionId { self.transaction }

    pub fn step<I, R>(mut self,
                      runtime : &mut crate::irq_runtime::BoardIrqRuntime<
                          I, BoardIrqOwner<R>>,
                      mmc_irq : GlobalIrq)
                      -> Result<ReadCoordinatorStepProgress,
                                ReadCoordinatorStepFailure>
    where I : crate::liointc::RegisterIo, R : RegisterIo
    {
        let (step, remaining, polls_completed) = match &mut *self.service {
            ReadCoordinatorState::Rechecking { recheck, .. } =>
                (recheck.step_in_place(runtime, mmc_irq),
                 recheck.remaining(),
                 recheck.polls_completed()),
            _ => unreachable!("service phase was validated before construction"),
        };
        match step {
            Ok(BoundedMmcReadRecheckStep::Pending) => {
                Ok(ReadCoordinatorStepProgress::Pending {
                    transaction : self.transaction,
                    remaining,
                    polls_completed,
                })
            },
            Ok(BoundedMmcReadRecheckStep::Terminal) => {
                let transaction = self.transaction;
                *self.service = ReadCoordinatorState::Terminal {
                    transaction, polls_completed,
                };
                Ok(ReadCoordinatorStepProgress::Terminal {
                    transaction, polls_completed,
                })
            },
            Ok(BoundedMmcReadRecheckStep::Timeout) => {
                let transaction = self.transaction;
                let cause = ReadRecoveryCause::Timeout { polls_completed };
                *self.service = ReadCoordinatorState::RecoveryPending {
                    transaction, cause,
                };
                Ok(ReadCoordinatorStepProgress::RecoveryPending {
                    transaction, cause,
                })
            },
            Err(error) => {
                if let BoundedMmcReadRecheckError::Recheck(error) = error {
                    let cause = ReadRecoveryCause::RecheckFault {
                        error, polls_completed, remaining,
                    };
                    *self.service = ReadCoordinatorState::RecoveryPending {
                        transaction : self.transaction, cause,
                    };
                    Ok(ReadCoordinatorStepProgress::RecoveryPending {
                        transaction : self.transaction, cause,
                    })
                } else {
                    Err(ReadCoordinatorStepFailure { error })
                }
            },
        }
    }
}

#[must_use = "claim the same-generation pair or drop the service to retry later"]
pub struct ReadTerminalService<'a> {
    #[cfg_attr(not(test), allow(dead_code))]
    service : RuntimeService<'a, ReadCoordinatorState>,
    transaction : ReadTransactionId,
}

impl ReadTerminalService<'_> {
    pub const fn transaction(&self) -> ReadTransactionId { self.transaction }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn claim_pair<I, R>(
        mut self,
        runtime : &mut crate::irq_runtime::BoardIrqRuntime<I, BoardIrqOwner<R>>,
        mmc_irq : GlobalIrq,
        dma_irq : GlobalIrq,
        armed : ArmedReadIrqs)
        -> Result<ReadyReadIrqPair, ReadTerminalClaimFailure>
    where I : crate::liointc::RegisterIo
    {
        if armed.transaction() != self.transaction {
            return Err(ReadTerminalClaimFailure {
                error : ReadTerminalClaimError::WrongTransaction {
                    expected : self.transaction,
                    actual : armed.transaction(),
                },
                armed,
            });
        }
        match take_pending_read_irq_pair(runtime, mmc_irq, dma_irq, armed) {
            Ok(pair) => {
                *self.service = ReadCoordinatorState::CompletionClaimed {
                    transaction : self.transaction,
                };
                Ok(pair)
            },
            Err(failure) => {
                let error = failure.error;
                Err(ReadTerminalClaimFailure {
                    error : ReadTerminalClaimError::Pair(error),
                    armed : failure.into_armed(),
                })
            },
        }
    }
}

#[must_use = "record a completion failure or drop the service to retry later"]
pub struct ReadClaimedCompletionService<'a> {
    service : RuntimeService<'a, ReadCoordinatorState>,
    transaction : ReadTransactionId,
}

impl ReadClaimedCompletionService<'_> {
    pub const fn transaction(&self) -> ReadTransactionId { self.transaction }

    pub fn record_failure(
        mut self,
        failure : crate::mmc::ReadCompletionFailure)
        -> ReadRecoveryCause {
        let cause = ReadRecoveryCause::CompletionFailure(failure);
        *self.service = ReadCoordinatorState::RecoveryPending {
            transaction : self.transaction,
            cause,
        };
        cause
    }

    pub fn finalize(
        mut self,
        evidence : ClaimedReadCompletionEvidence)
        -> Result<(), ReadCompletionFinalizeFailure> {
        if evidence.transaction() != self.transaction ||
           evidence.dma_transaction() != self.transaction
        {
            let actual = if evidence.transaction() != self.transaction {
                evidence.transaction()
            } else {
                evidence.dma_transaction()
            };
            return Err(ReadCompletionFinalizeFailure {
                error : ReadCoordinatorError::WrongTransaction {
                    expected : self.transaction, actual,
                },
                evidence,
            });
        }
        let actual = evidence.completion_evidence();
        let expected = crate::mmc::ReadCompletionEvidence {
            command_response_validated : true,
            data_finished : true,
            dma_finished : true,
        };
        if actual != expected {
            return Err(ReadCompletionFinalizeFailure {
                error : ReadCoordinatorError::InvalidCompletionEvidence(actual),
                evidence,
            });
        }
        *self.service = ReadCoordinatorState::CompletionFinalized {
            transaction : self.transaction,
        };
        Ok(())
    }
}

#[must_use = "retire the quiesced owners or drop the service to retry later"]
pub struct ReadRecoveryService<'a> {
    service : RuntimeService<'a, ReadCoordinatorState>,
    transaction : ReadTransactionId,
}

impl ReadRecoveryService<'_> {
    pub const fn transaction(&self) -> ReadTransactionId { self.transaction }

    /// Archive a receipt pair that was already removed from the runtime owner
    /// table and whose DMA mappings have subsequently returned to CPU ownership.
    pub fn archive_claimed(
        mut self,
        evidence : ClaimedReadRecoveryEvidence)
        -> Result<(), ReadClaimedRecoveryArchiveFailure> {
        let cause = match &*self.service {
            ReadCoordinatorState::RecoveryPending { cause, .. } => *cause,
            _ => unreachable!("recovery phase was validated before construction"),
        };
        if evidence.transaction() != self.transaction ||
           evidence.dma_transaction() != self.transaction
        {
            let actual = if evidence.transaction() != self.transaction {
                evidence.transaction()
            } else {
                evidence.dma_transaction()
            };
            return Err(ReadClaimedRecoveryArchiveFailure {
                error : ReadCoordinatorError::WrongTransaction {
                    expected : self.transaction, actual,
                },
                evidence,
            });
        }
        let actual = ReadRecoveryCause::CompletionFailure(evidence.failure());
        if cause != actual {
            return Err(ReadClaimedRecoveryArchiveFailure {
                error : ReadCoordinatorError::RecoveryCauseMismatch {
                    expected : cause, actual,
                },
                evidence,
            });
        }
        *self.service = ReadCoordinatorState::RecoveryRecorded(ReadRecoveryReport {
            transaction : self.transaction,
            cause,
            partial_mmc_interrupts : evidence.mmc_interrupts(),
            drained : DrainedReadIrqs { mmc : None, dma : None },
            claimed : Some(evidence),
        });
        Ok(())
    }

    /// Atomically retire software IRQ owners and publish their report into the
    /// coordinator slot. Both hardware interrupt sources remain masked.
    pub fn retire_and_record<I, R>(
        mut self,
        runtime : &mut crate::irq_runtime::BoardIrqRuntime<I, BoardIrqOwner<R>>,
        mmc_irq : GlobalIrq,
        dma_irq : GlobalIrq,
        quiesced : QuiescedReadIrqs)
        -> Result<(), ReadCoordinatorRecoveryFailure>
    where I : crate::liointc::RegisterIo
    {
        let cause = match &*self.service {
            ReadCoordinatorState::RecoveryPending { cause, .. } => *cause,
            _ => unreachable!("recovery phase was validated before construction"),
        };
        if quiesced.transaction() != self.transaction {
            return Err(ReadCoordinatorRecoveryFailure {
                error : ReadCoordinatorRecoveryError::WrongTransaction {
                    expected : self.transaction,
                    actual : quiesced.transaction(),
                },
                cause,
                quiesced,
            });
        }
        let report = match retire_quiesced_read_recovery(
            runtime, mmc_irq, dma_irq, quiesced, cause)
        {
            Ok(report) => report,
            Err(failure) => {
                let error = failure.error;
                let cause = failure.cause;
                return Err(ReadCoordinatorRecoveryFailure {
                    error : ReadCoordinatorRecoveryError::Retire(error),
                    cause,
                    quiesced : failure.into_quiesced(),
                });
            },
        };
        *self.service = ReadCoordinatorState::RecoveryRecorded(report);
        Ok(())
    }
}

impl Default for ReadCoordinatorSlot {
    fn default() -> Self { Self::new() }
}

pub struct ReadCoordinatorReservation<'a> {
    reservation : RuntimeReservation<'a, ReadCoordinatorState>,
    transaction : ReadTransactionId,
}

impl ReadCoordinatorReservation<'_> {
    pub fn commit(self) {
        self.reservation.commit(ReadCoordinatorState::Reserved {
            transaction : self.transaction,
        });
    }
}

/// Reservation carrying the production request metadata through commit.
#[must_use = "commit the request or drop it to restore the slot"]
pub struct ReadRequestReservation<'a> {
    reservation : ReadCoordinatorReservation<'a>,
    slot : &'a ReadCoordinatorSlot,
    request : ReadRequest,
}

impl<'a> ReadRequestReservation<'a> {
    pub fn commit(self) -> ReadRequestHandle<'a> {
        let Self { reservation, slot, request } = self;
        reservation.commit();
        ReadRequestHandle { slot, request }
    }
}

/// Stable worker-facing handle for a request already reserved in the slot.
pub struct ReadRequestHandle<'a> {
    slot : &'a ReadCoordinatorSlot,
    request : ReadRequest,
}

impl ReadRequestHandle<'_> {
    pub const fn transaction(&self) -> ReadTransactionId { self.request.transaction() }

    pub const fn block(&self) -> ReadBlockRequest { self.request.block() }

    pub fn snapshot(&self) -> Result<ReadCoordinatorSnapshot, ReadCoordinatorError> {
        self.slot.snapshot()
    }

    pub fn mark_published(&self, poll_budget : u16) -> Result<(), ReadCoordinatorError> {
        self.slot.mark_published(self.transaction(), poll_budget)
    }

    pub fn release(self) -> Result<(), ReadCoordinatorError> {
        self.slot.release(self.transaction())
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use api_v0::DriverResult;
    use crate::board_irq_owner::{DeferredApbDmaOwner, DrainedReadIrqs,
                                 MmcCommandOwner, MmcReadRecheckError,
                                 ReadIrqOwnerBinding};
    use crate::mmc::MmcIrqAckError;
    use dw_mmc::mmc::MmcError;

    #[derive(Default)]
    struct ModelLioIo;

    impl crate::liointc::RegisterIo for ModelLioIo {
        fn read32(&self, _address : usize) -> u32 { 0 }
        fn write32(&mut self, _address : usize, _value : u32) {}
        fn write8(&mut self, _address : usize, _value : u8) {}
    }

    #[derive(Default)]
    struct MockRegisters {
        status : u32,
        fail_read : bool,
        panic_read : bool,
    }

    #[derive(Default)]
    struct MockCoherency {
        fail_device : bool,
    }

    impl DmaCoherency for MockCoherency {
        fn sync_for_device(&mut self,
                           _region : DmaRegion,
                           _direction : DmaDirection)
                           -> DriverResult<()> {
            if self.fail_device { Err(DriverError::IoError) } else { Ok(()) }
        }

        fn sync_for_cpu(&mut self,
                        _region : DmaRegion,
                        _direction : DmaDirection)
                        -> DriverResult<()> { Ok(()) }
    }

    impl RegisterIo for MockRegisters {
        fn read32(&mut self, offset : usize) -> Result<u32, MmcError> {
            if self.panic_read { panic!("injected register panic"); }
            if self.fail_read { return Err(MmcError::RegisterOutOfRange); }
            if offset == 0x3c { Ok(self.status) } else { Err(MmcError::RegisterOutOfRange) }
        }

        fn write32(&mut self, _offset : usize, _value : u32) -> Result<(), MmcError> { Ok(()) }
    }

    fn transaction(raw : u64) -> ReadTransactionId {
        ReadTransactionId::new(raw).unwrap()
    }

    fn report(transaction : ReadTransactionId) -> ReadRecoveryReport {
        ReadRecoveryReport {
            transaction,
            cause : ReadRecoveryCause::Timeout { polls_completed : 2 },
            partial_mmc_interrupts : 1 << 6,
            drained : DrainedReadIrqs { mmc : None, dma : None },
            claimed : None,
        }
    }

    fn runtime(mmc_irq : GlobalIrq,
               dma_irq : GlobalIrq)
               -> crate::irq_runtime::BoardIrqRuntime<
                   ModelLioIo, BoardIrqOwner<MockRegisters>> {
        let bank0 = crate::liointc::LioIntc::new(ModelLioIo, 0, 0x1000, &[0x2000]).unwrap();
        let bank1 = crate::liointc::LioIntc::new(ModelLioIo, 1, 0x1040, &[0x2040]).unwrap();
        let mut runtime = crate::irq_runtime::BoardIrqRuntime::new(
            [Some(bank0), Some(bank1)], [None; 8]).unwrap();
        runtime.register(
            mmc_irq,
            BoardIrqOwner::MmcCommand(MmcCommandOwner::new(
                mmc_irq, MockRegisters::default())))
            .unwrap_or_else(|_| panic!("register MMC owner failed"));
        runtime.register(
            dma_irq,
            BoardIrqOwner::ApbDmaDeferred(DeferredApbDmaOwner::new(dma_irq)))
            .unwrap_or_else(|_| panic!("register DMA owner failed"));
        runtime
    }

    #[test]
    fn reservation_drop_reopens_and_live_slot_rejects_reentry() {
        let slot = ReadCoordinatorSlot::new();
        let current = transaction(1);
        let reservation = slot.reserve(current).unwrap();
        assert_eq!(slot.state(), DiagnosticSlotState::Reserved);
        assert_eq!(slot.reserve(current).err(),
                   Some(ReadCoordinatorError::Slot(SlotError::Reserved)));
        drop(reservation);
        slot.reserve(current).unwrap().commit();
        assert_eq!(slot.snapshot().unwrap().phase, ReadCoordinatorPhase::Reserved);
        assert_eq!(slot.reserve(transaction(2)).err(),
                   Some(ReadCoordinatorError::Slot(SlotError::AlreadyLive)));
    }

    #[test]
    fn production_request_handle_keeps_geometry_and_transaction_bound() {
        let slot = ReadCoordinatorSlot::new();
        let transaction = transaction(2);
        let block = ReadBlockRequest::new(8, 2, 512, crate::mmc::ReadAddressing::Block).unwrap();
        let request = ReadRequest::new(transaction, block);
        let handle = request.reserve(&slot).unwrap().commit();
        assert_eq!(handle.transaction(), transaction);
        assert_eq!(handle.block(), block);
        assert_eq!(handle.snapshot().unwrap().phase, ReadCoordinatorPhase::Reserved);
        handle.mark_published(3).unwrap();
        assert_eq!(slot.snapshot().unwrap().phase, ReadCoordinatorPhase::Published);
        handle.release().unwrap();
        assert_eq!(slot.state(), DiagnosticSlotState::Empty);
    }

    #[test]
    fn production_request_buffer_enforces_geometry_and_linear_dma_ownership() {
        let request = ReadRequest::new(
            transaction(4),
            ReadBlockRequest::new(0, 2, 512, crate::mmc::ReadAddressing::Block).unwrap());
        let region = DmaRegion::new(0x4000, 0x8000, 1024, 32, 32).unwrap();
        let mapping = DmaMapping::new(region, DmaDirection::FromDevice, MockCoherency::default());
        let mut buffer = request.bind_dma_buffer(mapping).unwrap();
        assert_eq!(buffer.request(), request);
        assert_eq!(buffer.region(), Ok(region));
        assert_eq!(buffer.prepare_for_device(), Ok(region));
        assert!(!buffer.is_cpu_owned());
        assert_eq!(buffer.region(), Err(ReadRequestBufferError::Dma(DriverError::InvalidParam)));
        assert_eq!(buffer.complete_from_device(), Ok(region));
        assert!(buffer.is_cpu_owned());

        let wrong_length = DmaRegion::new(0x4000, 0x8000, 512, 32, 32).unwrap();
        let wrong_mapping = DmaMapping::new(wrong_length, DmaDirection::FromDevice,
                                            MockCoherency::default());
        assert_eq!(request.bind_dma_buffer(wrong_mapping).err(),
                   Some(ReadRequestBufferError::WrongLength { expected : 1024, actual : 512 }));
    }

    fn dma_plan_fixture(request : ReadRequest) ->
                         (TransferPlan, DmaMapping<MockCoherency>, DmaMapping<MockCoherency>) {
        let descriptor_region = DmaRegion::new(0x4000, 0x10_000, 64, 32, 32).unwrap();
        let payload_region = DmaRegion::new(0x8000, 0x20_000,
                                             request.block.byte_length, 32, 32).unwrap();
        let descriptor = apbdma::HardwareDescriptor {
            memory_address_low : payload_region.physical_address() as u32,
            apb_address : 0x1fe2_c000,
            length_words : (request.block.byte_length / 4) as u32,
            step_length : 0,
            step_times : 1,
            command : 1 << 1,
            status : 0,
            next_address_low : 0,
            next_address_high : 0,
            memory_address_high : (payload_region.physical_address() >> 32) as u32,
            reserved : [0; 2],
        };
        let plan = TransferPlan {
            descriptor,
            descriptor_physical_address : descriptor_region.physical_address(),
            start_order : descriptor_region.physical_address() | 1 | 8,
            memory_physical_address : payload_region.physical_address(),
            byte_length : request.block.byte_length,
            direction : ApbDmaDirection::DeviceToMemory,
            invalidate_buffer_after : true,
            clean_descriptor_before : true,
        };
        (plan,
         DmaMapping::new(descriptor_region, DmaDirection::Bidirectional,
                         MockCoherency::default()),
         DmaMapping::new(payload_region, DmaDirection::FromDevice,
                         MockCoherency::default()))
    }

    #[test]
    fn production_request_dma_lease_binds_pair_before_prepare() {
        let request = ReadRequest::new(
            transaction(5),
            ReadBlockRequest::new(0, 2, 512, crate::mmc::ReadAddressing::Block).unwrap());
        let (plan, descriptor, payload) = dma_plan_fixture(request);
        let mut lease = ReadRequestDmaLease::bind(request, plan, descriptor, payload).unwrap();
        assert_eq!(lease.request(), request);
        let prepared = lease.prepare_session().unwrap();
        prepared.cancel().unwrap();
    }

    #[test]
    fn coordinator_tracks_published_and_paused_recheck_across_workers() {
        let slot = ReadCoordinatorSlot::new();
        let current = transaction(3);
        slot.reserve(current).unwrap().commit();
        assert_eq!(slot.mark_published(current, 4), Ok(()));
        let recheck = BoundedMmcReadRecheck::new(current, 4).unwrap();
        assert_eq!(slot.record_recheck(recheck).map_err(|failure| failure.error), Ok(()));
        assert_eq!(slot.snapshot().unwrap(), ReadCoordinatorSnapshot {
            transaction : current,
            phase : ReadCoordinatorPhase::Rechecking,
            poll_budget : Some(4),
            remaining : Some(4),
            polls_completed : Some(0),
            recovery_cause : None,
            partial_mmc_interrupts : None,
            has_mmc_receipt : false,
            has_dma_receipt : false,
        });
        assert_eq!(slot.mark_published(transaction(4), 4),
                   Err(ReadCoordinatorError::WrongTransaction {
                       expected : current, actual : transaction(4),
                   }));
        let failure = slot.record_recheck(BoundedMmcReadRecheck::new(current, 3).unwrap())
                          .err().expect("invalid progress accepted");
        assert_eq!(failure.error, ReadCoordinatorError::WrongPhase {
            expected : ReadCoordinatorPhase::Published,
            actual : ReadCoordinatorPhase::Rechecking,
        });
        assert_eq!(failure.recheck.remaining(), 3);
        assert_eq!(slot.release(transaction(4)),
                   Err(ReadCoordinatorError::WrongTransaction {
                       expected : current, actual : transaction(4),
                   }));
        assert_eq!(slot.state(), DiagnosticSlotState::Live);
        assert_eq!(slot.release(current), Ok(()));
        assert_eq!(slot.state(), DiagnosticSlotState::Empty);
    }

    #[test]
    fn recovery_report_is_linear_and_take_reopens_slot() {
        let slot = ReadCoordinatorSlot::new();
        let current = transaction(5);
        slot.reserve(current).unwrap().commit();
        slot.mark_published(current, 2).unwrap();
        let failure = slot.record_recovery(report(transaction(6))).unwrap_err();
        assert_eq!(failure.error, ReadCoordinatorError::WrongTransaction {
            expected : current, actual : transaction(6),
        });
        assert_eq!(failure.report.transaction, transaction(6));
        let fault = ReadRecoveryReport {
            transaction : current,
            cause : ReadRecoveryCause::RecheckFault {
                error : MmcReadRecheckError::Ack(MmcIrqAckError::UnknownPending(1 << 20)),
                polls_completed : 1,
                remaining : 1,
            },
            partial_mmc_interrupts : 1 << 6,
            drained : DrainedReadIrqs { mmc : None, dma : None },
            claimed : None,
        };
        slot.record_recovery(fault).unwrap_or_else(|_| panic!("report rejected"));
        let snapshot = slot.snapshot().unwrap();
        assert_eq!(snapshot.phase, ReadCoordinatorPhase::RecoveryRecorded);
        assert_eq!(snapshot.partial_mmc_interrupts, Some(1 << 6));
        assert_eq!(slot.release(current),
                   Err(ReadCoordinatorError::RecoveryMustBeTaken));
        let recovered = slot.take_recovery(current).unwrap();
        assert_eq!(recovered.transaction, current);
        assert_eq!(slot.state(), DiagnosticSlotState::Empty);
        slot.reserve(transaction(7)).unwrap().commit();
    }

    #[test]
    fn service_guard_keeps_slot_busy_and_drop_restores_live_token() {
        let slot = ReadCoordinatorSlot::new();
        let current = transaction(8);
        slot.reserve(current).unwrap().commit();
        slot.mark_published(current, 2).unwrap();
        slot.record_recheck(BoundedMmcReadRecheck::new(current, 2).unwrap())
            .unwrap_or_else(|_| panic!("recheck rejected"));
        let service = slot.service_recheck(current).unwrap();
        assert_eq!(service.transaction(), current);
        assert_eq!(slot.state(), DiagnosticSlotState::Servicing);
        assert_eq!(slot.snapshot(), Err(ReadCoordinatorError::Slot(SlotError::Busy)));
        assert!(matches!(slot.service_recheck(current),
                         Err(ReadCoordinatorError::Slot(SlotError::Busy))));
        drop(service);
        assert_eq!(slot.snapshot().unwrap().phase, ReadCoordinatorPhase::Rechecking);
    }

    #[test]
    fn service_steps_split_terminal_completion_without_exposing_vacant_slot() {
        let mmc_irq = GlobalIrq::from_bank_local(0, 31).unwrap();
        let dma_irq = GlobalIrq::from_bank_local(1, 13).unwrap();
        let current = transaction(9);
        let mut runtime = runtime(mmc_irq, dma_irq);
        let BoardIrqOwner::MmcCommand(mmc) = runtime.owner_mut(mmc_irq).unwrap() else {
            panic!("wrong MMC owner")
        };
        mmc.arm_read(current).unwrap();
        let slot = ReadCoordinatorSlot::new();
        slot.reserve(current).unwrap().commit();
        slot.mark_published(current, 3).unwrap();
        slot.record_recheck(BoundedMmcReadRecheck::new(current, 3).unwrap())
            .unwrap_or_else(|_| panic!("recheck rejected"));

        assert_eq!(slot.service_recheck(current).unwrap().step(&mut runtime, mmc_irq),
                   Ok(ReadCoordinatorStepProgress::Pending {
                       transaction : current, remaining : 2, polls_completed : 1,
                   }));
        let BoardIrqOwner::MmcCommand(mmc) = runtime.owner_mut(mmc_irq).unwrap() else {
            panic!("wrong MMC owner")
        };
        mmc.registers_mut().status = 1 << 6;
        assert_eq!(slot.service_recheck(current).unwrap().step(&mut runtime, mmc_irq),
                   Ok(ReadCoordinatorStepProgress::Pending {
                       transaction : current, remaining : 1, polls_completed : 2,
                   }));
        let BoardIrqOwner::MmcCommand(mmc) = runtime.owner_mut(mmc_irq).unwrap() else {
            panic!("wrong MMC owner")
        };
        mmc.registers_mut().status = 1;
        assert_eq!(slot.service_recheck(current).unwrap().step(&mut runtime, mmc_irq),
                   Ok(ReadCoordinatorStepProgress::Terminal {
                       transaction : current, polls_completed : 3,
                   }));
        assert_eq!(slot.snapshot().unwrap().phase, ReadCoordinatorPhase::Terminal);
        assert_eq!(slot.release(current), Ok(()));
    }

    #[test]
    fn service_converts_timeout_and_fault_to_matching_recovery_pending() {
        let mmc_irq = GlobalIrq::from_bank_local(0, 31).unwrap();
        let dma_irq = GlobalIrq::from_bank_local(1, 13).unwrap();
        let current = transaction(10);
        let mut runtime = runtime(mmc_irq, dma_irq);
        let BoardIrqOwner::MmcCommand(mmc) = runtime.owner_mut(mmc_irq).unwrap() else {
            panic!("wrong MMC owner")
        };
        mmc.arm_read(current).unwrap();
        let slot = ReadCoordinatorSlot::new();
        slot.reserve(current).unwrap().commit();
        slot.mark_published(current, 1).unwrap();
        slot.record_recheck(BoundedMmcReadRecheck::new(current, 1).unwrap())
            .unwrap_or_else(|_| panic!("recheck rejected"));
        let cause = ReadRecoveryCause::Timeout { polls_completed : 1 };
        assert_eq!(slot.service_recheck(current).unwrap().step(&mut runtime, mmc_irq),
                   Ok(ReadCoordinatorStepProgress::RecoveryPending {
                       transaction : current, cause,
                   }));
        assert_eq!(slot.release(current), Err(ReadCoordinatorError::RecoveryMustBeRecorded));
        let mismatch = slot.record_recovery(ReadRecoveryReport {
            transaction : current,
            cause : ReadRecoveryCause::Timeout { polls_completed : 2 },
            partial_mmc_interrupts : 0,
            drained : DrainedReadIrqs { mmc : None, dma : None },
            claimed : None,
        }).err().expect("mismatched recovery cause accepted");
        assert!(matches!(mismatch.error, ReadCoordinatorError::RecoveryCauseMismatch { .. }));
        slot.record_recovery(ReadRecoveryReport {
            transaction : current,
            cause,
            partial_mmc_interrupts : 0,
            drained : DrainedReadIrqs { mmc : None, dma : None },
            claimed : None,
        }).unwrap_or_else(|_| panic!("matching timeout report rejected"));
        let recovered = slot.take_recovery(current).unwrap();
        assert_eq!(recovered.cause, cause);

        let fault = transaction(11);
        let BoardIrqOwner::MmcCommand(mmc) = runtime.owner_mut(mmc_irq).unwrap() else {
            panic!("wrong MMC owner")
        };
        assert_eq!(mmc.read_binding(), Some(ReadIrqOwnerBinding::Armed(current)));
        mmc.disarm_read(current).unwrap();
        mmc.arm_read(fault).unwrap();
        mmc.registers_mut().status = 1 << 20;
        slot.reserve(fault).unwrap().commit();
        slot.mark_published(fault, 2).unwrap();
        slot.record_recheck(BoundedMmcReadRecheck::new(fault, 2).unwrap())
            .unwrap_or_else(|_| panic!("fault recheck rejected"));
        let progress = slot.service_recheck(fault).unwrap().step(&mut runtime, mmc_irq)
                           .unwrap();
        let ReadCoordinatorStepProgress::RecoveryPending { cause, .. } = progress else {
            panic!("unknown MMC status did not enter recovery")
        };
        assert_eq!(cause, ReadRecoveryCause::RecheckFault {
            error : MmcReadRecheckError::Ack(MmcIrqAckError::UnknownPending(1 << 20)),
            polls_completed : 0,
            remaining : 2,
        });
    }

    #[test]
    fn service_restores_token_after_retryable_generation_failure() {
        let mmc_irq = GlobalIrq::from_bank_local(0, 31).unwrap();
        let dma_irq = GlobalIrq::from_bank_local(1, 13).unwrap();
        let current = transaction(12);
        let bound = transaction(13);
        let mut runtime = runtime(mmc_irq, dma_irq);
        let BoardIrqOwner::MmcCommand(mmc) = runtime.owner_mut(mmc_irq).unwrap() else {
            panic!("wrong MMC owner")
        };
        mmc.arm_read(bound).unwrap();
        let slot = ReadCoordinatorSlot::new();
        slot.reserve(current).unwrap().commit();
        slot.mark_published(current, 2).unwrap();
        slot.record_recheck(BoundedMmcReadRecheck::new(current, 2).unwrap())
            .unwrap_or_else(|_| panic!("recheck rejected"));
        assert_eq!(slot.service_recheck(bound).err(),
                   Some(ReadCoordinatorError::WrongTransaction {
                       expected : current, actual : bound,
                   }));
        let failure = slot.service_recheck(current).unwrap()
                          .step(&mut runtime, mmc_irq).unwrap_err();
        assert_eq!(failure.error,
                   BoundedMmcReadRecheckError::Binding(
                       Some(ReadIrqOwnerBinding::Armed(bound))));
        let snapshot = slot.snapshot().unwrap();
        assert_eq!(snapshot.phase, ReadCoordinatorPhase::Rechecking);
        assert_eq!(snapshot.remaining, Some(2));
        assert_eq!(snapshot.polls_completed, Some(0));

        let BoardIrqOwner::MmcCommand(mmc) = runtime.owner_mut(mmc_irq).unwrap() else {
            panic!("wrong MMC owner")
        };
        mmc.disarm_read(bound).unwrap();
        mmc.arm_read(current).unwrap();
        mmc.registers_mut().status = (1 << 6) | 1;
        assert_eq!(slot.service_recheck(current).unwrap().step(&mut runtime, mmc_irq),
                   Ok(ReadCoordinatorStepProgress::Terminal {
                       transaction : current, polls_completed : 1,
                   }));
    }

    #[test]
    fn service_unwind_restores_live_slot_and_in_place_token() {
        let mmc_irq = GlobalIrq::from_bank_local(0, 31).unwrap();
        let dma_irq = GlobalIrq::from_bank_local(1, 13).unwrap();
        let current = transaction(14);
        let mut runtime = runtime(mmc_irq, dma_irq);
        let BoardIrqOwner::MmcCommand(mmc) = runtime.owner_mut(mmc_irq).unwrap() else {
            panic!("wrong MMC owner")
        };
        mmc.arm_read(current).unwrap();
        mmc.registers_mut().panic_read = true;
        let slot = ReadCoordinatorSlot::new();
        slot.reserve(current).unwrap().commit();
        slot.mark_published(current, 2).unwrap();
        slot.record_recheck(BoundedMmcReadRecheck::new(current, 2).unwrap())
            .unwrap_or_else(|_| panic!("recheck rejected"));

        let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let service = slot.service_recheck(current).unwrap();
            let _ = service.step(&mut runtime, mmc_irq);
        }));
        assert!(unwind.is_err());
        assert_eq!(slot.state(), DiagnosticSlotState::Live);
        let snapshot = slot.snapshot().unwrap();
        assert_eq!(snapshot.phase, ReadCoordinatorPhase::Rechecking);
        assert_eq!(snapshot.remaining, Some(2));
        assert_eq!(snapshot.polls_completed, Some(0));

        let BoardIrqOwner::MmcCommand(mmc) = runtime.owner_mut(mmc_irq).unwrap() else {
            panic!("wrong MMC owner")
        };
        mmc.registers_mut().panic_read = false;
        mmc.registers_mut().status = (1 << 6) | 1;
        assert_eq!(slot.service_recheck(current).unwrap().step(&mut runtime, mmc_irq),
                   Ok(ReadCoordinatorStepProgress::Terminal {
                       transaction : current, polls_completed : 1,
                   }));
    }

    #[test]
    fn fault_recovery_service_records_partial_snapshot_before_owner_drain() {
        let mmc_irq = GlobalIrq::from_bank_local(0, 31).unwrap();
        let dma_irq = GlobalIrq::from_bank_local(1, 13).unwrap();
        let current = transaction(15);
        let mut runtime = runtime(mmc_irq, dma_irq);
        let BoardIrqOwner::MmcCommand(mmc) = runtime.owner_mut(mmc_irq).unwrap() else {
            panic!("wrong MMC owner")
        };
        mmc.arm_read(current).unwrap();
        mmc.registers_mut().status = 1 << 6;
        let BoardIrqOwner::ApbDmaDeferred(dma) = runtime.owner_mut(dma_irq).unwrap() else {
            panic!("wrong DMA owner")
        };
        dma.arm_read(current).unwrap();
        let slot = ReadCoordinatorSlot::new();
        slot.reserve(current).unwrap().commit();
        slot.mark_published(current, 3).unwrap();
        slot.record_recheck(BoundedMmcReadRecheck::new(current, 3).unwrap())
            .unwrap_or_else(|_| panic!("recheck rejected"));
        assert_eq!(slot.service_recheck(current).unwrap().step(&mut runtime, mmc_irq),
                   Ok(ReadCoordinatorStepProgress::Pending {
                       transaction : current, remaining : 2, polls_completed : 1,
                   }));
        let BoardIrqOwner::MmcCommand(mmc) = runtime.owner_mut(mmc_irq).unwrap() else {
            panic!("wrong MMC owner")
        };
        mmc.registers_mut().status = 1 << 20;
        let progress = slot.service_recheck(current).unwrap().step(&mut runtime, mmc_irq)
                           .unwrap();
        let ReadCoordinatorStepProgress::RecoveryPending { cause, .. } = progress else {
            panic!("unknown status did not enter recovery")
        };
        let wrong = transaction(16);
        let failure = slot.service_recovery(current).unwrap()
            .retire_and_record(&mut runtime,
                               mmc_irq,
                               dma_irq,
                               QuiescedReadIrqs::fixture(wrong))
            .expect_err("wrong quiesced generation retired current owners");
        assert_eq!(failure.error, ReadCoordinatorRecoveryError::WrongTransaction {
            expected : current, actual : wrong,
        });
        assert_eq!(failure.cause, cause);
        assert_eq!(failure.into_quiesced().transaction(), wrong);
        assert_eq!(slot.snapshot().unwrap().phase, ReadCoordinatorPhase::RecoveryPending);
        let BoardIrqOwner::MmcCommand(mmc) = runtime.owner(mmc_irq).unwrap() else {
            panic!("wrong MMC owner")
        };
        assert_eq!(mmc.read_binding(), Some(ReadIrqOwnerBinding::Armed(current)));
        slot.service_recovery(current).unwrap()
            .retire_and_record(&mut runtime,
                               mmc_irq,
                               dma_irq,
                               QuiescedReadIrqs::fixture(current))
            .unwrap_or_else(|_| panic!("fault owners did not retire"));
        let snapshot = slot.snapshot().unwrap();
        assert_eq!(snapshot.phase, ReadCoordinatorPhase::RecoveryRecorded);
        assert_eq!(snapshot.recovery_cause, Some(cause));
        assert_eq!(snapshot.partial_mmc_interrupts, Some(1 << 6));
        assert!(!snapshot.has_mmc_receipt);
        assert!(!snapshot.has_dma_receipt);
        let report = slot.take_recovery(current).unwrap();
        assert_eq!(report.partial_mmc_interrupts, 1 << 6);
        let BoardIrqOwner::MmcCommand(mmc) = runtime.owner(mmc_irq).unwrap() else {
            panic!("wrong MMC owner")
        };
        assert_eq!(mmc.read_binding(), None);
        let BoardIrqOwner::ApbDmaDeferred(dma) = runtime.owner(dma_irq).unwrap() else {
            panic!("wrong DMA owner")
        };
        assert_eq!(dma.read_binding(), None);
    }
}

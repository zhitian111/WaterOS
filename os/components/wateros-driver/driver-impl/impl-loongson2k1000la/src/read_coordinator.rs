//! Production storage for one deferred MMC/APBDMA read coordinator.
//!
//! This module stores software lifecycle metadata only. It never publishes an
//! MMC command, starts DMA, accesses MMIO or rearms an interrupt source.
//! `UNVERIFIED_ON_HARDWARE`: worker scheduling and late-IRQ timing still need
//! validation on a physical 2K1000LA board before this slot can gate rearm.

use crate::{board_irq_owner::{BoundedMmcReadRecheck, ReadRecoveryCause,
                              ReadRecoveryReport, ReadTransactionId},
            diagnostic_slot::{DiagnosticRuntimeSlot, DiagnosticSlotState,
                              DrainError, RuntimeReservation, SlotError}};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadCoordinatorPhase {
    Reserved,
    Published,
    Rechecking,
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
    RecoveryMustBeTaken,
}

#[must_use = "retry recording or retain the linear recovery report"]
pub struct RecordRecoveryFailure {
    pub error : ReadCoordinatorError,
    pub report : ReadRecoveryReport,
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
        remaining : u16,
        polls_completed : u16,
    },
    RecoveryRecorded(ReadRecoveryReport),
}

impl ReadCoordinatorState {
    fn transaction(&self) -> ReadTransactionId {
        match self {
            Self::Reserved { transaction } |
            Self::Published { transaction, .. } |
            Self::Rechecking { transaction, .. } => *transaction,
            Self::RecoveryRecorded(report) => report.transaction,
        }
    }

    fn phase(&self) -> ReadCoordinatorPhase {
        match self {
            Self::Reserved { .. } => ReadCoordinatorPhase::Reserved,
            Self::Published { .. } => ReadCoordinatorPhase::Published,
            Self::Rechecking { .. } => ReadCoordinatorPhase::Rechecking,
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
            Self::Rechecking {
                poll_budget, remaining, polls_completed, ..
            } => {
                snapshot.poll_budget = Some(*poll_budget);
                snapshot.remaining = Some(*remaining);
                snapshot.polls_completed = Some(*polls_completed);
            },
            Self::RecoveryRecorded(report) => {
                snapshot.recovery_cause = Some(report.cause);
                snapshot.partial_mmc_interrupts = Some(report.partial_mmc_interrupts);
                snapshot.has_mmc_receipt = report.drained.mmc.is_some();
                snapshot.has_dma_receipt = report.drained.dma.is_some();
            },
            Self::Reserved { .. } => {},
        }
        snapshot
    }
}

pub struct ReadCoordinatorSlot {
    inner : DiagnosticRuntimeSlot<ReadCoordinatorState>,
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
                          recheck : &BoundedMmcReadRecheck)
                          -> Result<(), ReadCoordinatorError> {
        let transaction = recheck.transaction();
        self.with_state(transaction, |state| {
            let poll_budget = match state {
                ReadCoordinatorState::Published { poll_budget, .. } |
                ReadCoordinatorState::Rechecking { poll_budget, .. } => *poll_budget,
                _ => {
                    return Err(ReadCoordinatorError::WrongPhase {
                        expected : ReadCoordinatorPhase::Published,
                        actual : state.phase(),
                    });
                },
            };
            if recheck.remaining().checked_add(recheck.polls_completed()) != Some(poll_budget) {
                return Err(ReadCoordinatorError::InvalidPollProgress);
            }
            *state = ReadCoordinatorState::Rechecking {
                transaction,
                poll_budget,
                remaining : recheck.remaining(),
                polls_completed : recheck.polls_completed(),
            };
            Ok(())
        })
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
                         ReadCoordinatorState::Rechecking { .. })
            {
                return Err(ReadCoordinatorError::WrongPhase {
                    expected : ReadCoordinatorPhase::Rechecking,
                    actual : state.phase(),
                });
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board_irq_owner::{DrainedReadIrqs, MmcReadRecheckError};
    use crate::mmc::MmcIrqAckError;

    fn transaction(raw : u64) -> ReadTransactionId {
        ReadTransactionId::new(raw).unwrap()
    }

    fn report(transaction : ReadTransactionId) -> ReadRecoveryReport {
        ReadRecoveryReport {
            transaction,
            cause : ReadRecoveryCause::Timeout { polls_completed : 2 },
            partial_mmc_interrupts : 1 << 6,
            drained : DrainedReadIrqs { mmc : None, dma : None },
        }
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
    fn coordinator_tracks_published_and_paused_recheck_across_workers() {
        let slot = ReadCoordinatorSlot::new();
        let current = transaction(3);
        slot.reserve(current).unwrap().commit();
        assert_eq!(slot.mark_published(current, 4), Ok(()));
        let recheck = BoundedMmcReadRecheck::new(current, 4).unwrap();
        assert_eq!(slot.record_recheck(&recheck), Ok(()));
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
        assert_eq!(slot.record_recheck(&BoundedMmcReadRecheck::new(current, 3).unwrap()),
                   Err(ReadCoordinatorError::InvalidPollProgress));
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
}

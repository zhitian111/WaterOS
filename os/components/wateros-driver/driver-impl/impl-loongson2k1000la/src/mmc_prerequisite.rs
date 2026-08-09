//! Aggregate MMC prerequisite evidence without activating the controller.
//!
//! Topology ownership, instantaneous observations and hardware-verified tokens
//! remain distinct. Current physical paths are `UNVERIFIED_ON_HARDWARE`, so a
//! normal diagnosis cannot form [`ControllerPrerequisiteProof`].

use crate::{
    clock::ClockError,
    diagnostic_irq::DiagnosticIrqSnapshot,
    diagnostic_slot::DiagnosticSlotState,
    gpio::CardDetectSnapshot,
    mmc::{ControllerClockReady, PrerequisiteStatus},
    mmc_diagnostic::{CardDetectDiagnosis, Diagnosis},
    pinctrl::{PinctrlError, PinctrlState, Ready},
};

const MMC_GLOBAL_IRQ : u8 = 31;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateStatus {
    Satisfied,
    ObservedOnly,
    UnverifiedOnHardware,
    Blocked,
    Missing,
    Unsupported,
    Error,
}

pub fn gate_code(status : GateStatus) -> &'static str {
    match status {
        GateStatus::Satisfied => "satisfied",
        GateStatus::ObservedOnly => "observed-only",
        GateStatus::UnverifiedOnHardware => "unverified-hardware",
        GateStatus::Blocked => "blocked",
        GateStatus::Missing => "missing",
        GateStatus::Unsupported => "unsupported",
        GateStatus::Error => "error",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrqObservation {
    Unavailable,
    Transitional,
    LiveObserved,
    LiveDegraded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrerequisiteReport {
    pub clock : GateStatus,
    pub vmmc : GateStatus,
    pub vqmmc : GateStatus,
    pub pinctrl : GateStatus,
    pub card_detect : GateStatus,
    pub irq : GateStatus,
}

impl PrerequisiteReport {
    pub fn can_form_proof(&self) -> bool {
        [self.clock,
         self.vmmc,
         self.vqmmc,
         self.pinctrl,
         self.card_detect,
         self.irq].into_iter()
                  .all(|status| status == GateStatus::Satisfied)
    }
}

pub fn irq_observation(snapshot : DiagnosticIrqSnapshot) -> IrqObservation {
    match (snapshot.slot_state, snapshot.runtime) {
        (DiagnosticSlotState::Empty, None) => IrqObservation::Unavailable,
        (DiagnosticSlotState::Live, Some(runtime)) => {
            let mmc_configured = runtime.configured_sources & (1u64 << MMC_GLOBAL_IRQ) != 0;
            let clean = runtime.status_poll_failures
                               .iter()
                               .all(Option::is_none);
            if mmc_configured && clean {
                IrqObservation::LiveObserved
            } else {
                IrqObservation::LiveDegraded
            }
        }
        _ => IrqObservation::Transitional,
    }
}

fn topology_power(status : PrerequisiteStatus) -> GateStatus {
    match status {
        PrerequisiteStatus::ReadyByTopology |
        PrerequisiteStatus::ImplicitBoardSupply |
        PrerequisiteStatus::FirmwareMaintained => GateStatus::UnverifiedOnHardware,
        PrerequisiteStatus::RequiresDriver => GateStatus::Blocked,
        PrerequisiteStatus::Missing => GateStatus::Missing,
        PrerequisiteStatus::UnsupportedProvider => GateStatus::Unsupported,
    }
}

pub fn report(diagnosis : Diagnosis, irq : IrqObservation) -> PrerequisiteReport {
    let clock = match diagnosis.clock {
        Ok(_) => GateStatus::ObservedOnly,
        Err(ClockError::UnsupportedProvider) => GateStatus::Unsupported,
        Err(_) => GateStatus::Error,
    };
    let pinctrl = match diagnosis.pinctrl {
        Ok(snapshot)
            if PinctrlState::new(snapshot).classify()
                                          .is_ok() =>
        {
            GateStatus::Satisfied
        }
        Ok(_) => GateStatus::Blocked,
        Err(PinctrlError::Missing) => GateStatus::Missing,
        Err(PinctrlError::UnsupportedProvider) => GateStatus::Unsupported,
        Err(PinctrlError::Io) => GateStatus::Error,
    };
    let card_detect = match diagnosis.card_detect {
        CardDetectDiagnosis::NonRemovable => GateStatus::Satisfied,
        CardDetectDiagnosis::Gpio(Ok(CardDetectSnapshot { card_present: true, .. })) => {
            GateStatus::Satisfied
        }
        CardDetectDiagnosis::Gpio(Ok(_)) => GateStatus::Blocked,
        CardDetectDiagnosis::Gpio(Err(crate::gpio::GpioError::UnsupportedProvider)) => {
            GateStatus::Unsupported
        }
        CardDetectDiagnosis::Gpio(Err(_)) => GateStatus::Error,
        CardDetectDiagnosis::FirmwareMaintainedBroken => GateStatus::UnverifiedOnHardware,
        CardDetectDiagnosis::NativeUnavailable => GateStatus::Missing,
    };
    let irq = match irq {
        IrqObservation::Unavailable => GateStatus::Missing,
        IrqObservation::Transitional | IrqObservation::LiveDegraded => GateStatus::Blocked,
        IrqObservation::LiveObserved => GateStatus::ObservedOnly,
    };
    PrerequisiteReport { clock,
                         vmmc : topology_power(diagnosis.plan
                                                        .prerequisites
                                                        .vmmc),
                         vqmmc : topology_power(diagnosis.plan
                                                         .prerequisites
                                                         .vqmmc),
                         pinctrl,
                         card_detect,
                         irq }
}

/// Hardware-verified clock-control evidence. No normal constructor exists.
pub struct ClockReady {
    _controller : ControllerClockReady,
}

impl ClockReady {
    /// # Safety
    /// Caller must verify clock programming, stability and target rate on the
    /// physical board, not merely obtain a read-only rate snapshot.
    pub unsafe fn assume_verified(controller : ControllerClockReady) -> Self {
        Self { _controller : controller }
    }
}

/// Hardware-verified rail and sequencing evidence. No normal constructor exists.
pub struct PowerReady {
    _private : (),
}

impl PowerReady {
    /// # Safety
    /// Caller must verify both physical rails and required sequencing.
    pub const unsafe fn assume_verified() -> Self { Self { _private : () } }
}

pub struct CardReady {
    _private : (),
}

impl CardReady {
    pub fn from_diagnosis(value : CardDetectDiagnosis) -> Option<Self> {
        match value {
            CardDetectDiagnosis::NonRemovable |
            CardDetectDiagnosis::Gpio(Ok(CardDetectSnapshot { card_present: true, .. })) => {
                Some(Self { _private : () })
            }
            _ => None,
        }
    }
}

/// Hardware-verified interrupt delivery evidence. No normal constructor exists.
pub struct IrqReady {
    _private : (),
}

impl IrqReady {
    /// # Safety
    /// Caller must verify real MMC interrupt delivery, device ack, masking and
    /// rearm on the physical board; a live diagnostic runtime is insufficient.
    pub const unsafe fn assume_verified() -> Self { Self { _private : () } }
}

/// Opaque aggregate prerequisite proof. This is still not a data-path token.
pub struct ControllerPrerequisiteProof {
    _clock : ClockReady,
    _power : PowerReady,
    _pinctrl : PinctrlState<Ready>,
    _card : CardReady,
    _irq : IrqReady,
}

pub fn assemble_proof(clock : ClockReady,
                      power : PowerReady,
                      pinctrl : PinctrlState<Ready>,
                      card : CardReady,
                      irq : IrqReady)
                      -> ControllerPrerequisiteProof {
    ControllerPrerequisiteProof { _clock : clock,
                                  _power : power,
                                  _pinctrl : pinctrl,
                                  _card : card,
                                  _irq : irq }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        gpio::CardDetectSnapshot,
        irq_runtime::{RuntimeDiagnosticSnapshot, ServiceCounters},
        mmc::{ActivationBlocker, BringUpPlan, PrerequisitePlan},
        pinctrl::{self, RegisterIo},
        topology::{MmcPinctrlDescription, PinctrlProvider},
    };
    use api_v0::MmioRegion;

    struct Pins(u32);

    struct Clocks;

    struct ControllerClockRegisters {
        values : [u32; 26],
    }

    impl crate::mmc::RegisterIo for ControllerClockRegisters {
        fn read32(&mut self, offset : usize) -> Result<u32, dw_mmc::mmc::MmcError> {
            self.values
                .get(offset / 4)
                .copied()
                .ok_or(dw_mmc::mmc::MmcError::RegisterOutOfRange)
        }

        fn write32(&mut self, offset : usize, value : u32) -> Result<(), dw_mmc::mmc::MmcError> {
            *self.values
                 .get_mut(offset / 4)
                 .ok_or(dw_mmc::mmc::MmcError::RegisterOutOfRange)? = value;
            Ok(())
        }
    }

    struct Delay;

    impl crate::mmc::ResetDelay for Delay {
        fn delay_milliseconds(&mut self, milliseconds : u32) {
            assert_eq!(milliseconds, 10);
        }
    }

    impl crate::clock::RegisterIo for Clocks {
        fn read32(&mut self, offset : usize) -> Result<u32, ClockError> {
            assert_eq!(offset, 0x28);
            Ok(2 << 22)
        }

        fn read64(&mut self, offset : usize) -> Result<u64, ClockError> {
            match offset {
                0x20 => Ok((10u64 << 32) | (2u64 << 26)),
                0x50 => Ok(3 << 20),
                _ => panic!("unexpected clock offset {offset:#x}"),
            }
        }
    }

    impl RegisterIo for Pins {
        fn read32(&mut self, _offset : usize) -> Result<u32, PinctrlError> { Ok(self.0) }
    }

    fn pin_snapshot(raw : u32) -> crate::pinctrl::PinctrlSnapshot {
        pinctrl::snapshot(Some(MmcPinctrlDescription {
                              state_phandle : 1,
                              provider : PinctrlProvider::Loongson2k {
                                  mmio : MmioRegion { base : 4, size : 0x18 },
                              },
                          }),
                          &mut Pins(raw)).unwrap()
    }

    fn clock_snapshot() -> crate::clock::ClockSnapshot {
        crate::clock::ClockSnapshot { dc_pll_raw : (10u64 << 32) | (2u64 << 26),
                                      gmac_div_raw : 2 << 22,
                                      apb_scale_raw : 3 << 20,
                                      reference_hz : 100_000_000,
                                      dc_pll_hz : 500_000_000,
                                      gmac_hz : 250_000_000,
                                      apb_hz : 125_000_000 }
    }

    fn diagnosis() -> Diagnosis {
        Diagnosis {
            plan : BringUpPlan {
                controller_mmio : MmioRegion { base : 0x1000, size : 0x68 },
                auxiliary_mmio : MmioRegion { base : 0x2000, size : 8 },
                bus_width : 4,
                prerequisites : PrerequisitePlan {
                    clock : PrerequisiteStatus::RequiresDriver,
                    vmmc : PrerequisiteStatus::ImplicitBoardSupply,
                    vqmmc : PrerequisiteStatus::ReadyByTopology,
                    pinctrl : PrerequisiteStatus::RequiresDriver,
                    card_detect : PrerequisiteStatus::RequiresDriver,
                },
                blockers : [ActivationBlocker::DataPathUnavailable,
                            ActivationBlocker::ExternalDmaExecutorUnavailable,
                            ActivationBlocker::ClockControlUnavailable,
                            ActivationBlocker::PowerSequencingUnavailable,
                            ActivationBlocker::CardDetectUnavailable,
                            ActivationBlocker::PinControlUnavailable,
                            ActivationBlocker::InterruptPathUnverified],
            },
            clock : Ok(clock_snapshot()),
            pinctrl : Ok(pin_snapshot(1 << 20)),
            card_detect : CardDetectDiagnosis::Gpio(Ok(CardDetectSnapshot {
                direction_raw : 1 << 22,
                input_raw : 0,
                pin : 22,
                active_low : true,
                level_high : false,
                card_present : true,
            })),
        }
    }

    #[test]
    fn healthy_observations_still_refuse_to_claim_hardware_proof() {
        let report = report(diagnosis(),
                            IrqObservation::LiveObserved);
        assert_eq!(report.clock, GateStatus::ObservedOnly);
        assert_eq!(report.vmmc,
                   GateStatus::UnverifiedOnHardware);
        assert_eq!(report.vqmmc,
                   GateStatus::UnverifiedOnHardware);
        assert_eq!(report.pinctrl, GateStatus::Satisfied);
        assert_eq!(report.card_detect,
                   GateStatus::Satisfied);
        assert_eq!(report.irq, GateStatus::ObservedOnly);
        assert!(!report.can_form_proof());
    }

    #[test]
    fn fault_matrix_classifies_each_independent_gate() {
        let mut value = diagnosis();
        value.clock = Err(ClockError::Io);
        value.pinctrl = Ok(pin_snapshot(0));
        value.card_detect =
            CardDetectDiagnosis::Gpio(Ok(CardDetectSnapshot { card_present : false,
                                                              ..match diagnosis().card_detect {
                                                                  CardDetectDiagnosis::Gpio(Ok(snapshot)) => {
                                                                      snapshot
                                                                  }
                                                                  _ => unreachable!(),
                                                              } }));
        value.plan
             .prerequisites
             .vmmc = PrerequisiteStatus::RequiresDriver;
        value.plan
             .prerequisites
             .vqmmc = PrerequisiteStatus::UnsupportedProvider;
        let report = report(value, IrqObservation::LiveDegraded);
        assert_eq!(report.clock, GateStatus::Error);
        assert_eq!(report.vmmc, GateStatus::Blocked);
        assert_eq!(report.vqmmc, GateStatus::Unsupported);
        assert_eq!(report.pinctrl, GateStatus::Blocked);
        assert_eq!(report.card_detect, GateStatus::Blocked);
        assert_eq!(report.irq, GateStatus::Blocked);
    }

    #[test]
    fn irq_snapshot_is_observation_not_delivery_proof() {
        let live = DiagnosticIrqSnapshot {
            slot_state : DiagnosticSlotState::Live,
            runtime : Some(RuntimeDiagnosticSnapshot {
                configured_sources : 1 << MMC_GLOBAL_IRQ,
                parent_lines : 1,
                service : ServiceCounters::default(),
                status_poll_failures : [None, None],
            }),
        };
        assert_eq!(irq_observation(live),
                   IrqObservation::LiveObserved);
        assert_eq!(irq_observation(DiagnosticIrqSnapshot { slot_state:
                                                               DiagnosticSlotState::Empty,
                                                           runtime : None }),
                   IrqObservation::Unavailable);
    }

    #[test]
    fn aggregate_proof_requires_every_typed_token() {
        let pins = PinctrlState::new(pin_snapshot(1 << 20)).classify()
                                                           .unwrap();
        let card = CardReady::from_diagnosis(diagnosis().card_detect).unwrap();
        let consistent = crate::clock::snapshot_consistent(&mut Clocks, 100_000_000).unwrap();
        let plan = crate::mmc::ControllerClockPlan::from_parent(consistent, 25_000_000).unwrap();
        let preflight_authority =
            unsafe { crate::mmc::HostPreflightAuthority::assume_board_verified() };
        let host = crate::mmc::Host::new(ControllerClockRegisters { values : [0; 26] },
                                         2).preflight(&mut Delay, &preflight_authority)
                                           .unwrap();
        let clock_authority =
            unsafe { crate::mmc::ControllerClockAuthority::assume_board_verified() };
        let mut guard = loop {
            match crate::mmc::try_begin_clock_transaction() {
                Ok(guard) => break guard,
                Err(crate::mmc::ClockTransactionBusy) => core::hint::spin_loop(),
            }
        };
        let (host, controller) =
            host.configure_controller_clock(plan, &clock_authority, &mut guard)
                .unwrap();
        drop(guard);
        // SAFETY: these are pure host fixtures with no claim about real hardware.
        let proof = assemble_proof(unsafe { ClockReady::assume_verified(controller) },
                                   unsafe { PowerReady::assume_verified() },
                                   pins,
                                   card,
                                   unsafe { IrqReady::assume_verified() });
        let _authorized = host.authorize(proof);
    }
}

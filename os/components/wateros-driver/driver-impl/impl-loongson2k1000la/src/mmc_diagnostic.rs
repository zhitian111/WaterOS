//! Combined read-only MMC prerequisite diagnostics.
//!
//! A successful snapshot is evidence about one instant, not permission to
//! activate the host. Every [`Diagnosis`] retains the original bring-up plan,
//! whose blockers remain authoritative. Physical reads are
//! `UNVERIFIED_ON_HARDWARE` until exercised on a 2K1000 board.

use crate::{
    clock::{self, ClockError, ClockSnapshot},
    gpio::{self, CardDetectSnapshot, GpioError},
    mmc::{self, BringUpPlan, PlanError, PrerequisiteStatus},
    mmc_prerequisite::{self, IrqObservation},
    pinctrl::{self, PinctrlError, PinctrlSnapshot},
    topology::{CardDetect, MmcDescription},
};
use alloc::{format, string::String};
use core::{fmt::{self, Write}, sync::atomic::{AtomicBool, Ordering}};

/// Non-blocking exclusion for explicit physical diagnostic reads.
pub struct DiagnosticGate {
    busy : AtomicBool,
}

impl DiagnosticGate {
    pub const fn new() -> Self { Self { busy : AtomicBool::new(false) } }

    pub fn try_enter(&self) -> Result<DiagnosticGuard<'_>, GateError> {
        self.busy
            .compare_exchange(false,
                              true,
                              Ordering::AcqRel,
                              Ordering::Acquire)
            .map(|_| DiagnosticGuard { gate : self })
            .map_err(|_| GateError::Busy)
    }
}

impl Default for DiagnosticGate {
    fn default() -> Self { Self::new() }
}

pub struct DiagnosticGuard<'a> {
    gate : &'a DiagnosticGate,
}

impl Drop for DiagnosticGuard<'_> {
    fn drop(&mut self) {
        self.gate
            .busy
            .store(false, Ordering::Release);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateError {
    Busy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardDetectDiagnosis {
    /// No removable-card observation is necessary.
    NonRemovable,
    /// Firmware or board policy owns detection; no level was observed.
    FirmwareMaintainedBroken,
    /// Native controller detection has no safe read-only model yet.
    NativeUnavailable,
    Gpio(Result<CardDetectSnapshot, GpioError>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Diagnosis {
    pub plan : BringUpPlan,
    pub clock : Result<ClockSnapshot, ClockError>,
    pub pinctrl : Result<PinctrlSnapshot, PinctrlError>,
    pub card_detect : CardDetectDiagnosis,
}

fn clock_error_code(error : ClockError) -> &'static str {
    match error {
        ClockError::Io => "io",
        ClockError::ZeroReference => "zero-reference",
        ClockError::ZeroPllMultiplier => "zero-pll-multiplier",
        ClockError::ZeroPllDivisor => "zero-pll-divisor",
        ClockError::RateOverflow => "rate-overflow",
        ClockError::Inconsistent => "inconsistent",
        ClockError::UnsupportedProvider => "unsupported-provider",
    }
}

fn gpio_error_code(error : GpioError) -> &'static str {
    match error {
        GpioError::Io => "io",
        GpioError::UnsupportedProvider => "unsupported-provider",
        GpioError::PinOutOfRange => "pin-out-of-range",
        GpioError::NotInput => "not-input",
    }
}

fn prerequisite_code(status : PrerequisiteStatus) -> &'static str {
    match status {
        PrerequisiteStatus::ReadyByTopology => "topology-ready",
        PrerequisiteStatus::ImplicitBoardSupply => "implicit-board-supply",
        PrerequisiteStatus::FirmwareMaintained => "firmware-maintained",
        PrerequisiteStatus::RequiresDriver => "requires-driver",
        PrerequisiteStatus::Missing => "missing",
        PrerequisiteStatus::UnsupportedProvider => "unsupported-provider",
    }
}

fn pinctrl_error_code(error : PinctrlError) -> &'static str {
    match error {
        PinctrlError::Io => "io",
        PinctrlError::Missing => "missing",
        PinctrlError::UnsupportedProvider => "unsupported-provider",
    }
}

fn post_stage_code(stage : mmc::CommandPostObservationStage) -> &'static str {
    match stage {
        mmc::CommandPostObservationStage::ReadArgument => "read-carg",
        mmc::CommandPostObservationStage::ReadControl => "read-cctl",
        mmc::CommandPostObservationStage::ReadCommandStatus => "read-csts",
        mmc::CommandPostObservationStage::ReadDataStatus => "read-dsts",
        mmc::CommandPostObservationStage::ReadInterrupts => "read-int",
    }
}

fn write_optional_hex(output : &mut impl Write, value : Option<u32>) -> fmt::Result {
    match value {
        Some(value) => write!(output, "{value:#x}"),
        None => output.write_str("na"),
    }
}

/// Append a stable, allocation-free controller post-observation fragment.
pub fn write_command_post(
    output : &mut impl Write,
    post : Result<mmc::CommandPostSnapshot, mmc::CommandPostObservationFailure>)
    -> fmt::Result {
    match post {
        Ok(post) => write!(output,
                           "controller=ok carg={:#x} cctl={:#x} csts={:#x} dsts={:#x} int={:#x} \
                            idle={} clean={} int_known={:#x} int_unknown={:#x}",
                           post.argument,
                           post.control,
                           post.command_status,
                           post.data_status,
                           post.interrupts,
                           u8::from(post.command_status & (1 << 8) == 0 &&
                                    post.data_status & 3 == 0),
                           u8::from(post.argument == 0 && post.control == 0),
                           post.interrupts & 0x3FF,
                           post.interrupts & !0x3FF),
        Err(failure) => {
            write!(output, "controller=error:{} carg=", post_stage_code(failure.stage))?;
            write_optional_hex(output, failure.argument)?;
            output.write_str(" cctl=")?;
            write_optional_hex(output, failure.control)?;
            output.write_str(" csts=")?;
            write_optional_hex(output, failure.command_status)?;
            output.write_str(" dsts=")?;
            write_optional_hex(output, failure.data_status)?;
            output.write_str(" int=na")
        }
    }
}

fn response_code(response : mmc::ResponseType) -> &'static str {
    match response {
        mmc::ResponseType::None => "none",
        mmc::ResponseType::Short => "short",
        mmc::ResponseType::Long => "long",
    }
}

fn command_stage_code(stage : mmc::CommandStage) -> &'static str {
    use mmc::CommandStage::*;
    match stage {
        ClearInterrupts => "clear-int",
        WriteArgument => "write-carg",
        StartCommand => "start",
        PollInterrupts => "poll-int",
        CommandTimeout => "command-timeout",
        ResponseCrc => "response-crc",
        PollTimeout => "poll-timeout",
        AcknowledgeCompletion => "ack-completion",
        ReadResponse0 => "read-rsp0",
        ReadResponse1 => "read-rsp1",
        ReadResponse2 => "read-rsp2",
        ReadResponse3 => "read-rsp3",
        CleanupArgument => "cleanup-carg",
        CleanupControl => "cleanup-cctl",
        RevalidateCommandStatus => "revalidate-csts",
        RevalidateDataStatus => "revalidate-dsts",
        RevalidateInterrupts => "revalidate-int",
        RevalidateBusy => "revalidate-busy",
        RevalidateUnknownInterrupt => "revalidate-unknown-int",
        RevalidateClearInterrupts => "revalidate-clear-int",
        RevalidateInterruptReadback => "revalidate-int-readback",
        RevalidateInterruptStillPending => "revalidate-int-pending",
        RevalidateCleanupArgument => "revalidate-cleanup-carg",
        RevalidateCleanupControl => "revalidate-cleanup-cctl",
        RevalidateArgumentReadback => "revalidate-carg-readback",
        RevalidateArgumentMismatch => "revalidate-carg-mismatch",
        RevalidateControlReadback => "revalidate-cctl-readback",
        RevalidateControlMismatch => "revalidate-cctl-mismatch",
    }
}

/// Append stable bounded command-trace fields without allocating.
pub fn write_command_trace(output : &mut impl Write, trace : mmc::CommandTrace) -> fmt::Result {
    write!(output,
           "trace=present cmd={} arg={:#x} response={} validation=unchecked cctl=",
           trace.command_index,
           trace.argument,
           response_code(trace.response))?;
    write_optional_hex(output, trace.programmed_control)?;
    write!(output,
           " samples={} dropped={} int_union={:#x} rsp_mask={:#x} cleanup={}/{} outcome=",
           trace.interrupt_sample_count,
           trace.dropped_interrupt_samples,
           trace.interrupt_union,
           trace.response_read_mask,
           u8::from(trace.cleanup_argument_written),
           u8::from(trace.cleanup_control_written))?;
    match trace.outcome {
        mmc::CommandTraceOutcome::InFlight => output.write_str("in-flight"),
        mmc::CommandTraceOutcome::Completed => output.write_str("completed"),
        mmc::CommandTraceOutcome::Failed(stage) => {
            write!(output, "failed:{}", command_stage_code(stage))
        }
    }
}

/// Append a stable assessment fragment; it never represents authorization.
pub fn write_command_assessment(output : &mut impl Write,
                                assessment : mmc::CommandValidationAssessment)
                                -> fmt::Result {
    let disposition = match assessment.disposition {
        mmc::CommandEvidenceDisposition::ObservedOnly => "observed-only",
        mmc::CommandEvidenceDisposition::IncompleteTrace => "incomplete-trace",
        mmc::CommandEvidenceDisposition::UnsafeState => "unsafe-state",
    };
    write!(output,
           "assessment={} completed={} trace_complete={} idle={} clean={} int_known={:#x} \
            int_unknown={:#x}",
           disposition,
           u8::from(assessment.command_completed),
           u8::from(assessment.trace_complete),
           u8::from(assessment.controller_idle),
           u8::from(assessment.command_registers_clean),
           assessment.known_interrupts,
           assessment.unknown_interrupts)
}

/// Stable single-line representation used by the remote development monitor.
pub fn format_diagnosis(diagnosis : Diagnosis) -> String {
    format_diagnosis_with_observation(diagnosis, IrqObservation::Unavailable)
}

/// Stable single-line representation including the current software IRQ view.
pub fn format_diagnosis_with_irq(diagnosis : Diagnosis,
                                 irq : crate::diagnostic_irq::DiagnosticIrqSnapshot)
                                 -> String {
    format_diagnosis_with_observation(diagnosis,
                                      mmc_prerequisite::irq_observation(irq))
}

/// Stable remote-monitor line with an explicitly requested controller
/// post-state observation. No command trace exists because diagnosis is read-only.
pub fn format_diagnosis_with_irq_and_post(
    diagnosis : Diagnosis,
    irq : crate::diagnostic_irq::DiagnosticIrqSnapshot,
    post : Result<mmc::CommandPostSnapshot, mmc::CommandPostObservationFailure>)
    -> String {
    let mut output = format_diagnosis_with_irq(diagnosis, irq);
    output.truncate(output.len().saturating_sub(2));
    output.push(' ');
    write_command_post(&mut output, post).expect("String formatting is infallible");
    output.push_str(" trace=none assessment=unavailable\r\n");
    output
}

fn format_diagnosis_with_observation(diagnosis : Diagnosis, irq : IrqObservation) -> String {
    let gates = mmc_prerequisite::report(diagnosis, irq);
    let clock = match diagnosis.clock {
        Ok(snapshot) => {
            format!("clock=ok ref_hz={} pll_raw={:#x} gmac_raw={:#x} apb_raw={:#x} apb_hz={}",
                    snapshot.reference_hz,
                    snapshot.dc_pll_raw,
                    snapshot.gmac_div_raw,
                    snapshot.apb_scale_raw,
                    snapshot.apb_hz)
        }
        Err(error) => format!("clock=error:{}",
                              clock_error_code(error)),
    };
    let card = match diagnosis.card_detect {
        CardDetectDiagnosis::NonRemovable => String::from("card=non-removable"),
        CardDetectDiagnosis::FirmwareMaintainedBroken => {
            String::from("card=firmware-maintained-broken")
        }
        CardDetectDiagnosis::NativeUnavailable => String::from("card=native-unavailable"),
        CardDetectDiagnosis::Gpio(Ok(snapshot)) => {
            format!("card=gpio dir_raw={:#x} input_raw={:#x} pin={} active_low={} level_high={} \
                     present={}",
                    snapshot.direction_raw,
                    snapshot.input_raw,
                    snapshot.pin,
                    u8::from(snapshot.active_low),
                    u8::from(snapshot.level_high),
                    u8::from(snapshot.card_present))
        }
        CardDetectDiagnosis::Gpio(Err(error)) => {
            format!("card=gpio-error:{}",
                    gpio_error_code(error))
        }
    };
    let pinmux = match diagnosis.pinctrl {
        Ok(snapshot) => format!("pinmux=ok raw={:#x} sdio={} card_gpio={} ready={}",
                               snapshot.mux_raw(),
                               u8::from(snapshot.sdio_selected()),
                               u8::from(snapshot.card_detect_gpio_selected()),
                               u8::from(pinctrl::PinctrlState::<pinctrl::Observed>::new(snapshot)
                                            .classify()
                                            .is_ok())),
        Err(error) => format!("pinmux=error:{}",
                              pinctrl_error_code(error)),
    };
    format!("ls2k-mmc {} vmmc={} vqmmc={} pinctrl={} {} {} \
             gates=clock:{},vmmc:{},vqmmc:{},pinctrl:{},card:{},irq:{} proof={} can_activate={} \
             blockers={}\r\n",
            clock,
            prerequisite_code(diagnosis.plan
                                       .prerequisites
                                       .vmmc),
            prerequisite_code(diagnosis.plan
                                       .prerequisites
                                       .vqmmc),
            prerequisite_code(diagnosis.plan
                                       .prerequisites
                                       .pinctrl),
            pinmux,
            card,
            mmc_prerequisite::gate_code(gates.clock),
            mmc_prerequisite::gate_code(gates.vmmc),
            mmc_prerequisite::gate_code(gates.vqmmc),
            mmc_prerequisite::gate_code(gates.pinctrl),
            mmc_prerequisite::gate_code(gates.card_detect),
            mmc_prerequisite::gate_code(gates.irq),
            u8::from(gates.can_form_proof()),
            u8::from(diagnosis.plan
                              .can_activate()),
            diagnosis.plan
                     .blockers
                     .len())
}

/// Collect clock and card-detect evidence without changing hardware state.
///
/// Clock and GPIO backends are independent so host tests can prove access
/// order and short-circuit behavior. Errors are retained in the diagnosis;
/// only an invalid static bring-up plan prevents a result.
pub fn diagnose<C : clock::RegisterIo, G : gpio::RegisterIo, P : pinctrl::RegisterIo>(
    description : &MmcDescription,
    clock_registers : &mut C,
    gpio_registers : &mut G,
    pinctrl_registers : &mut P)
    -> Result<Diagnosis, PlanError> {
    let plan = mmc::plan(description)?;
    let clock = clock::snapshot_provider_consistent(description.clock_provider,
                                                    clock_registers).map(|value| value.snapshot())
                                                                    .map_err(|recovery| {
                                                                        recovery.error
                                                                                .unwrap_or(ClockError::Inconsistent)
                                                                    });
    let pinctrl = pinctrl::snapshot(description.pinctrl, pinctrl_registers);
    let card_detect = match &description.card_detect {
        CardDetect::NonRemovable => CardDetectDiagnosis::NonRemovable,
        CardDetect::Broken => CardDetectDiagnosis::FirmwareMaintainedBroken,
        CardDetect::Native => CardDetectDiagnosis::NativeUnavailable,
        CardDetect::Gpio(line) => {
            CardDetectDiagnosis::Gpio(gpio::card_detect_snapshot(line, gpio_registers))
        }
    };
    Ok(Diagnosis { plan,
                   clock,
                   pinctrl,
                   card_detect })
}

#[cfg(target_arch = "loongarch64")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VolatileDiagnosisError {
    Plan(PlanError),
    ClockBackend(ClockError),
    GpioBackend(GpioError),
    PinctrlBackend(PinctrlError),
    ControllerBackend(dw_mmc::mmc::MmcError),
}

#[cfg(target_arch = "loongarch64")]
pub struct VolatileDiagnosis {
    pub diagnosis : Diagnosis,
    pub controller_post:
        Result<mmc::CommandPostSnapshot, mmc::CommandPostObservationFailure>,
}

#[cfg(target_arch = "loongarch64")]
enum TargetClockRegisters {
    Volatile(clock::VolatileRegisters),
    Unsupported,
}

#[cfg(target_arch = "loongarch64")]
impl clock::RegisterIo for TargetClockRegisters {
    fn read32(&mut self, offset : usize) -> Result<u32, ClockError> {
        match self {
            Self::Volatile(registers) => registers.read32(offset),
            Self::Unsupported => Err(ClockError::UnsupportedProvider),
        }
    }

    fn read64(&mut self, offset : usize) -> Result<u64, ClockError> {
        match self {
            Self::Volatile(registers) => registers.read64(offset),
            Self::Unsupported => Err(ClockError::UnsupportedProvider),
        }
    }
}

#[cfg(target_arch = "loongarch64")]
enum TargetGpioRegisters {
    Volatile(gpio::VolatileRegisters),
    Unused,
}

#[cfg(target_arch = "loongarch64")]
impl gpio::RegisterIo for TargetGpioRegisters {
    fn read64(&mut self, offset : usize) -> Result<u64, GpioError> {
        match self {
            Self::Volatile(registers) => registers.read64(offset),
            Self::Unused => Err(GpioError::UnsupportedProvider),
        }
    }
}

#[cfg(target_arch = "loongarch64")]
enum TargetPinctrlRegisters {
    Volatile(pinctrl::VolatileRegisters),
    Unused,
}

#[cfg(target_arch = "loongarch64")]
impl pinctrl::RegisterIo for TargetPinctrlRegisters {
    fn read32(&mut self, offset : usize) -> Result<u32, PinctrlError> {
        match self {
            Self::Volatile(registers) => registers.read32(offset),
            Self::Unused => Err(PinctrlError::UnsupportedProvider),
        }
    }
}

/// Explicit physical-MMIO diagnostic entry point; never called by machine init.
///
/// # Safety
/// All topology MMIO regions must be mapped device memory and exclusively
/// available for these reads. Register behavior is `UNVERIFIED_ON_HARDWARE`.
#[cfg(target_arch = "loongarch64")]
pub unsafe fn diagnose_volatile(description : &MmcDescription)
                                -> Result<VolatileDiagnosis, VolatileDiagnosisError> {
    use crate::topology::{GpioProvider, MmcClockProvider, PinctrlProvider};

    mmc::plan(description).map_err(VolatileDiagnosisError::Plan)?;
    let mut clock_registers = match description.clock_provider {
        MmcClockProvider::Loongson2k { mmio, .. } => {
            // SAFETY: delegated to this function's caller.
            let registers = unsafe { clock::VolatileRegisters::new(mmio.base, mmio.size) }
                .map_err(VolatileDiagnosisError::ClockBackend)?;
            TargetClockRegisters::Volatile(registers)
        }
        MmcClockProvider::Unsupported { .. } => TargetClockRegisters::Unsupported,
    };

    let mut gpio_registers = match &description.card_detect {
        CardDetect::Gpio(line) => match line.provider {
            GpioProvider::Loongson2k1000 { mmio, .. } => {
                // SAFETY: delegated to this function's caller.
                let registers = unsafe { gpio::VolatileRegisters::new(mmio.base, mmio.size) }
                    .map_err(VolatileDiagnosisError::GpioBackend)?;
                TargetGpioRegisters::Volatile(registers)
            }
            GpioProvider::Unsupported { .. } => TargetGpioRegisters::Unused,
        },
        _ => TargetGpioRegisters::Unused,
    };
    let mut pinctrl_registers = match description.pinctrl
                                                 .map(|state| state.provider)
    {
        Some(PinctrlProvider::Loongson2k { mmio }) => {
            // SAFETY: delegated to this function's caller.
            let registers = unsafe { pinctrl::VolatileRegisters::new(mmio.base, mmio.size) }
                .map_err(VolatileDiagnosisError::PinctrlBackend)?;
            TargetPinctrlRegisters::Volatile(registers)
        }
        None | Some(PinctrlProvider::Unsupported) => TargetPinctrlRegisters::Unused,
    };
    // SAFETY: delegated to this function's caller. The post observer performs
    // only fixed-order volatile reads and never constructs a Host.
    let mut controller_registers = unsafe {
        mmc::VolatileRegisters::from_region(description.controller_mmio)
    }.map_err(VolatileDiagnosisError::ControllerBackend)?;

    let diagnosis = diagnose(description,
                             &mut clock_registers,
                             &mut gpio_registers,
                             &mut pinctrl_registers).map_err(VolatileDiagnosisError::Plan)?;
    let controller_post = mmc::observe_command_post_state(&mut controller_registers);
    Ok(VolatileDiagnosis { diagnosis,
                           controller_post })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topology::{
        GpioLineDescription, GpioProvider, InterruptSpec, MmcClockProvider, MmcPinctrlDescription,
        NamedResource, PinctrlProvider, ResourceSpecifier, SupplyDescription,
    };
    use alloc::{vec, vec::Vec};
    use api_v0::MmioRegion;

    struct FixedBuffer<const N : usize> {
        bytes : [u8; N],
        len : usize,
    }

    impl<const N : usize> FixedBuffer<N> {
        fn new() -> Self { Self { bytes : [0; N],
                                 len : 0 } }

        fn text(&self) -> &str { core::str::from_utf8(&self.bytes[..self.len]).unwrap() }
    }

    impl<const N : usize> core::fmt::Write for FixedBuffer<N> {
        fn write_str(&mut self, value : &str) -> core::fmt::Result {
            let end = self.len.checked_add(value.len())
                              .ok_or(core::fmt::Error)?;
            if end > N {
                return Err(core::fmt::Error);
            }
            self.bytes[self.len..end].copy_from_slice(value.as_bytes());
            self.len = end;
            Ok(())
        }
    }

    #[test]
    fn diagnostic_gate_rejects_reentry_and_reopens_on_drop() {
        let gate = DiagnosticGate::new();
        let guard = gate.try_enter()
                        .unwrap();
        assert!(matches!(gate.try_enter(), Err(GateError::Busy)));
        drop(guard);
        assert!(gate.try_enter()
                    .is_ok());
    }

    #[test]
    fn command_evidence_format_is_stable_and_bounded() {
        let post = mmc::CommandPostSnapshot { argument : 0,
                                              control : 0,
                                              command_status : 0,
                                              data_status : 0,
                                              interrupts : 0 };
        let mut output = FixedBuffer::<256>::new();
        write_command_post(&mut output, Ok(post)).unwrap();
        assert_eq!(output.text(),
                   "controller=ok carg=0x0 cctl=0x0 csts=0x0 dsts=0x0 int=0x0 idle=1 \
                    clean=1 int_known=0x0 int_unknown=0x0");

        let trace = mmc::CommandTrace { command_index : 8,
                                        argument : 0x1AA,
                                        response : mmc::ResponseType::Short,
                                        validation : mmc::ResponseValidation::Unchecked,
                                        programmed_control : Some(0x348),
                                        interrupt_samples : [0; mmc::COMMAND_TRACE_CAPACITY],
                                        interrupt_sample_count : 2,
                                        dropped_interrupt_samples : 1,
                                        interrupt_union : 0x140,
                                        response_read_mask : 1,
                                        cleanup_argument_written : true,
                                        cleanup_control_written : false,
                                        outcome:
                                            mmc::CommandTraceOutcome::Failed(
                                                mmc::CommandStage::CleanupControl) };
        let mut output = FixedBuffer::<256>::new();
        write_command_trace(&mut output, trace).unwrap();
        assert_eq!(output.text(),
                   "trace=present cmd=8 arg=0x1aa response=short validation=unchecked cctl=0x348 \
                    samples=2 dropped=1 int_union=0x140 rsp_mask=0x1 cleanup=1/0 \
                    outcome=failed:cleanup-cctl");

        let assessment = mmc::assess_command_validation(trace, post);
        let mut output = FixedBuffer::<192>::new();
        write_command_assessment(&mut output, assessment).unwrap();
        assert_eq!(output.text(),
                   "assessment=incomplete-trace completed=0 trace_complete=0 idle=1 clean=1 \
                    int_known=0x0 int_unknown=0x0");

        let mut too_small = FixedBuffer::<8>::new();
        assert_eq!(write_command_post(&mut too_small, Ok(post)),
                   Err(core::fmt::Error));
    }

    #[test]
    fn command_post_failure_format_retains_partial_fields() {
        let failure = mmc::CommandPostObservationFailure {
            stage : mmc::CommandPostObservationStage::ReadDataStatus,
            error : dw_mmc::mmc::MmcError::RegisterOutOfRange,
            argument : Some(0),
            control : Some(0),
            command_status : Some(1 << 8),
            data_status : None,
        };
        let mut output = FixedBuffer::<160>::new();
        write_command_post(&mut output, Err(failure)).unwrap();
        assert_eq!(output.text(),
                   "controller=error:read-dsts carg=0x0 cctl=0x0 csts=0x100 dsts=na int=na");
    }

    #[derive(Default)]
    struct ClockModel {
        reads : Vec<(usize, usize)>,
        fail : bool,
        change_after_first : bool,
        gmac_reads : usize,
    }

    impl clock::RegisterIo for ClockModel {
        fn read32(&mut self, offset : usize) -> Result<u32, ClockError> {
            self.reads
                .push((offset, 4));
            if self.fail {
                Err(ClockError::Io)
            } else {
                self.gmac_reads += 1;
                Ok(if self.change_after_first && self.gmac_reads > 1 {
                       3 << 22
                   } else {
                       2 << 22
                   })
            }
        }

        fn read64(&mut self, offset : usize) -> Result<u64, ClockError> {
            self.reads
                .push((offset, 8));
            if self.fail {
                Err(ClockError::Io)
            } else if offset == 0x20 {
                Ok((40u64 << 32) | (4u64 << 26))
            } else {
                Ok(3u64 << 20)
            }
        }
    }

    #[derive(Default)]
    struct GpioModel {
        reads : Vec<usize>,
        direction : u64,
        input : u64,
    }

    impl gpio::RegisterIo for GpioModel {
        fn read64(&mut self, offset : usize) -> Result<u64, GpioError> {
            self.reads
                .push(offset);
            if offset == 0 {
                Ok(self.direction)
            } else {
                Ok(self.input)
            }
        }
    }

    #[derive(Default)]
    struct PinctrlModel {
        raw : u32,
        reads : usize,
        fail : bool,
    }

    impl pinctrl::RegisterIo for PinctrlModel {
        fn read32(&mut self, offset : usize) -> Result<u32, PinctrlError> {
            assert_eq!(offset, 0);
            self.reads += 1;
            if self.fail {
                Err(PinctrlError::Io)
            } else {
                Ok(self.raw)
            }
        }
    }

    fn pinctrl_state(provider : PinctrlProvider) -> MmcPinctrlDescription {
        MmcPinctrlDescription { state_phandle : 4,
                                provider }
    }

    fn description(card_detect : CardDetect) -> MmcDescription {
        MmcDescription { controller_mmio : MmioRegion { base : 0x1FE0_C000,
                                                        size : 0x68 },
                         auxiliary_mmio : Some(MmioRegion { base : 0x1FE0_048C,
                                                            size : 4 }),
                         interrupt : InterruptSpec { parent_phandle : 1,
                                                     cells : [31, 4, 0, 0],
                                                     cell_count : 2 },
                         clocks : vec![NamedResource { name : Some("apb".into()),
                                          specifier : ResourceSpecifier {
                                              provider_phandle : 2,
                                              args : vec![12],
                                          } }],
                         clock_provider:
                             MmcClockProvider::Loongson2k { mmio : MmioRegion { base:
                                                                                    0x1FE0_0480,
                                                                                size : 0x58 },
                                                            reference_hz : 100_000_000 },
                         dma : None,
                         bus_width : 4,
                         pinctrl : None,
                         card_detect,
                         vmmc_supply : None::<SupplyDescription>,
                         vqmmc_supply : None::<SupplyDescription> }
    }

    fn gpio_line() -> GpioLineDescription {
        GpioLineDescription { specifier : ResourceSpecifier { provider_phandle : 3,
                                                              args : vec![22, 1] },
                              provider:
                                  GpioProvider::Loongson2k1000 { mmio : MmioRegion { base:
                                                                                         0x1FE0_0500,
                                                                                     size:
                                                                                         0x38 },
                                                                 ngpios : 64 },
                              pin : 22,
                              active_low : true }
    }

    #[test]
    fn combines_clock_and_active_low_card_evidence_without_unblocking_plan() {
        let mut description = description(CardDetect::Gpio(gpio_line()));
        description.pinctrl =
            Some(pinctrl_state(PinctrlProvider::Loongson2k { mmio : MmioRegion { base:
                                                                                     0x1FE0_0420,
                                                                                 size:
                                                                                     0x18 } }));
        let mut clocks = ClockModel::default();
        let mut gpios = GpioModel { direction : 1 << 22,
                                    input : 0,
                                    ..Default::default() };
        let mut pins = PinctrlModel { raw : 1 << 20,
                                      ..Default::default() };
        let result = diagnose(&description,
                              &mut clocks,
                              &mut gpios,
                              &mut pins).unwrap();

        assert_eq!(result.clock
                         .unwrap()
                         .apb_hz,
                   250_000_000);
        assert_eq!(result.card_detect,
                   CardDetectDiagnosis::Gpio(Ok(CardDetectSnapshot { direction_raw : 1 << 22,
                                                                     input_raw : 0,
                                                                     pin : 22,
                                                                     active_low : true,
                                                                     level_high : false,
                                                                     card_present : true })));
        assert!(!result.plan
                       .can_activate());
        assert_eq!(clocks.reads, vec![(0x20, 8),
                                      (0x28, 4),
                                      (0x50, 8),
                                      (0x20, 8),
                                      (0x28, 4),
                                      (0x50, 8)]);
        assert_eq!(gpios.reads, vec![0, 0x20]);
        assert_eq!(pins.reads, 1);
        assert_eq!(format_diagnosis(result),
                   "ls2k-mmc clock=ok ref_hz=100000000 pll_raw=0x2810000000 gmac_raw=0x800000 \
                    apb_raw=0x300000 apb_hz=250000000 vmmc=implicit-board-supply \
                    vqmmc=implicit-board-supply pinctrl=requires-driver pinmux=ok raw=0x100000 \
                    sdio=1 card_gpio=1 ready=1 card=gpio dir_raw=0x400000 input_raw=0x0 pin=22 \
                    active_low=1 level_high=0 present=1 \
                    gates=clock:observed-only,vmmc:unverified-hardware,vqmmc:unverified-hardware,\
                    pinctrl:satisfied,card:satisfied,irq:missing proof=0 can_activate=0 \
                    blockers=7\r\n");
    }

    #[test]
    fn retains_independent_read_failures_and_all_blockers() {
        let mut description = description(CardDetect::Gpio(gpio_line()));
        description.pinctrl =
            Some(pinctrl_state(PinctrlProvider::Loongson2k { mmio : MmioRegion { base:
                                                                                     0x1FE0_0420,
                                                                                 size:
                                                                                     0x18 } }));
        let mut clocks = ClockModel { fail : true,
                                      ..Default::default() };
        let mut gpios = GpioModel::default();
        let mut pins = PinctrlModel { fail : true,
                                      ..Default::default() };
        let result = diagnose(&description,
                              &mut clocks,
                              &mut gpios,
                              &mut pins).unwrap();

        assert_eq!(result.clock, Err(ClockError::Io));
        assert_eq!(result.pinctrl, Err(PinctrlError::Io));
        assert_eq!(result.card_detect,
                   CardDetectDiagnosis::Gpio(Err(GpioError::NotInput)));
        assert_eq!(result.plan
                         .blockers
                         .len(),
                   7);
        assert!(!result.plan
                       .can_activate());
        assert_eq!(format_diagnosis(result),
                   "ls2k-mmc clock=error:io vmmc=implicit-board-supply \
                    vqmmc=implicit-board-supply pinctrl=requires-driver pinmux=error:io \
                    card=gpio-error:not-input \
                    gates=clock:error,vmmc:unverified-hardware,vqmmc:unverified-hardware,pinctrl:\
                    error,card:error,irq:missing proof=0 can_activate=0 blockers=7\r\n");
    }

    #[test]
    fn rejects_mixed_generation_clock_diagnosis() {
        let description = description(CardDetect::NonRemovable);
        let mut clocks = ClockModel { change_after_first : true,
                                      ..Default::default() };
        let mut gpios = GpioModel::default();
        let mut pins = PinctrlModel::default();
        let result = diagnose(&description,
                              &mut clocks,
                              &mut gpios,
                              &mut pins).unwrap();

        assert_eq!(result.clock,
                   Err(ClockError::Inconsistent));
        assert!(format_diagnosis(result).contains("clock=error:inconsistent"));
        assert_eq!(clocks.reads.len(), 6);
    }

    #[test]
    fn topology_only_card_modes_never_read_gpio() {
        for (card_detect, expected) in
            [(CardDetect::NonRemovable, CardDetectDiagnosis::NonRemovable),
             (CardDetect::Broken, CardDetectDiagnosis::FirmwareMaintainedBroken),
             (CardDetect::Native, CardDetectDiagnosis::NativeUnavailable)]
        {
            let mut clocks = ClockModel::default();
            let mut gpios = GpioModel::default();
            let mut pins = PinctrlModel::default();
            let result = diagnose(&description(card_detect),
                                  &mut clocks,
                                  &mut gpios,
                                  &mut pins).unwrap();
            assert_eq!(result.card_detect, expected);
            assert!(gpios.reads
                         .is_empty());
        }
    }

    #[test]
    fn unsupported_providers_fail_without_access() {
        let mut description = description(CardDetect::Gpio(GpioLineDescription {
            provider : GpioProvider::Unsupported { phandle : 3 },
            ..gpio_line()
        }));
        description.clock_provider = MmcClockProvider::Unsupported { phandle : 2 };
        description.pinctrl = Some(pinctrl_state(PinctrlProvider::Unsupported));
        let mut clocks = ClockModel::default();
        let mut gpios = GpioModel::default();
        let mut pins = PinctrlModel::default();
        let result = diagnose(&description,
                              &mut clocks,
                              &mut gpios,
                              &mut pins).unwrap();

        assert_eq!(result.clock,
                   Err(ClockError::UnsupportedProvider));
        assert_eq!(result.pinctrl,
                   Err(PinctrlError::UnsupportedProvider));
        assert_eq!(pins.reads, 0);
        assert_eq!(result.card_detect,
                   CardDetectDiagnosis::Gpio(Err(GpioError::UnsupportedProvider)));
        assert!(clocks.reads
                      .is_empty());
        assert!(gpios.reads
                     .is_empty());
        assert!(!result.plan
                       .can_activate());
    }

    #[test]
    fn invalid_static_plan_prevents_every_read() {
        let mut description = description(CardDetect::Gpio(gpio_line()));
        description.controller_mmio
                   .size = 4;
        let mut clocks = ClockModel::default();
        let mut gpios = GpioModel::default();
        let mut pins = PinctrlModel::default();
        assert_eq!(diagnose(&description,
                            &mut clocks,
                            &mut gpios,
                            &mut pins),
                   Err(PlanError::ControllerWindowTooSmall));
        assert!(clocks.reads
                      .is_empty());
        assert!(gpios.reads
                     .is_empty());
        assert_eq!(pins.reads, 0);
    }
}

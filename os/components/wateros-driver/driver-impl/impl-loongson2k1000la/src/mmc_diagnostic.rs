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
use core::sync::atomic::{AtomicBool, Ordering};

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
    let clock = clock::snapshot_provider(description.clock_provider,
                                         clock_registers);
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
                                -> Result<Diagnosis, VolatileDiagnosisError> {
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

    diagnose(description,
             &mut clock_registers,
             &mut gpio_registers,
             &mut pinctrl_registers).map_err(VolatileDiagnosisError::Plan)
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

    #[derive(Default)]
    struct ClockModel {
        reads : Vec<(usize, usize)>,
        fail : bool,
    }

    impl clock::RegisterIo for ClockModel {
        fn read32(&mut self, offset : usize) -> Result<u32, ClockError> {
            self.reads
                .push((offset, 4));
            if self.fail {
                Err(ClockError::Io)
            } else {
                Ok(2 << 22)
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

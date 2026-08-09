//! Deferred 2K1000LA MMC bring-up planning.
//!
//! Linux's dedicated `loongson2-mmc` driver proves this is not a DesignWare
//! register layout. The second DT register is an APB-DMA routing register, not
//! a FIFO window. WaterOS reuses [`dw_mmc::sd`] only as an SD protocol layer.

use crate::{
    clock::ConsistentClockSnapshot,
    irq_domain::{AcknowledgedIrq, DeviceAckedIrq, GlobalIrq, IrqDisposition},
    mmc_prerequisite::ControllerPrerequisiteProof,
    topology::{
        CardDetect, FixedSupplyControl, MmcClockProvider, MmcDescription, PinctrlProvider,
        SupplyDescription, SupplyProvider,
    },
};
use api_v0::MmioRegion;
use core::marker::PhantomData;
use core::sync::atomic::{AtomicBool, Ordering};
use dw_mmc::mmc::MmcError;

/// Minimum documented main-register window from the upstream 2K1000 DTS.
const MIN_CONTROLLER_WINDOW : usize = 0x68;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanError {
    ControllerWindowTooSmall,
    MissingAuxiliaryWindow,
    MissingClock,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationBlocker {
    DataPathUnavailable,
    ExternalDmaExecutorUnavailable,
    ClockControlUnavailable,
    PowerSequencingUnavailable,
    CardDetectUnavailable,
    PinControlUnavailable,
    InterruptPathUnverified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrerequisiteStatus {
    ReadyByTopology,
    /// Upstream MMC treats the absent optional supply as board-wired power.
    ImplicitBoardSupply,
    FirmwareMaintained,
    RequiresDriver,
    Missing,
    UnsupportedProvider,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrerequisitePlan {
    /// These values classify DT ownership; they are not hardware observations.
    pub clock : PrerequisiteStatus,
    pub vmmc : PrerequisiteStatus,
    pub vqmmc : PrerequisiteStatus,
    pub pinctrl : PrerequisiteStatus,
    pub card_detect : PrerequisiteStatus,
}

/// Validated resource snapshot for future conservative PIO activation.
///
/// This is deliberately not convertible to `DwMmc`: the controller is a
/// distinct Loongson design. Activation remains disabled until its DMA path and
/// board prerequisites exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BringUpPlan {
    pub controller_mmio : MmioRegion,
    pub auxiliary_mmio : MmioRegion,
    pub bus_width : u8,
    pub prerequisites : PrerequisitePlan,
    pub blockers : [ActivationBlocker; 7],
}

impl BringUpPlan {
    pub const fn can_activate(&self) -> bool { false }
}

pub fn plan(description : &MmcDescription) -> Result<BringUpPlan, PlanError> {
    if description.controller_mmio
                  .size <
       MIN_CONTROLLER_WINDOW
    {
        return Err(PlanError::ControllerWindowTooSmall);
    }
    let auxiliary_mmio = description.auxiliary_mmio
                                    .filter(|region| region.size >= 4)
                                    .ok_or(PlanError::MissingAuxiliaryWindow)?;
    if description.clocks
                  .len() !=
       1
    {
        return Err(PlanError::MissingClock);
    }
    let clock = match description.clock_provider {
        MmcClockProvider::Loongson2k { .. } => PrerequisiteStatus::RequiresDriver,
        MmcClockProvider::Unsupported { .. } => PrerequisiteStatus::UnsupportedProvider,
    };
    let supply = |description : Option<SupplyDescription>| match description {
        None => PrerequisiteStatus::ImplicitBoardSupply,
        Some(SupplyDescription { provider:
                                     SupplyProvider::Fixed { control:
                                                                 FixedSupplyControl::None,
                                                             .. },
                                 .. }) => PrerequisiteStatus::ReadyByTopology,
        Some(SupplyDescription { provider:
                                     SupplyProvider::Fixed { control:
                                                                 FixedSupplyControl::Gpio,
                                                             .. },
                                 .. }) => PrerequisiteStatus::RequiresDriver,
        Some(SupplyDescription { provider: SupplyProvider::Unsupported,
                                 .. }) => PrerequisiteStatus::UnsupportedProvider,
    };
    let card_detect = match description.card_detect {
        CardDetect::NonRemovable => PrerequisiteStatus::ReadyByTopology,
        CardDetect::Gpio(_) | CardDetect::Native => PrerequisiteStatus::RequiresDriver,
        CardDetect::Broken => PrerequisiteStatus::FirmwareMaintained,
    };
    let pinctrl = match description.pinctrl {
        None => PrerequisiteStatus::Missing,
        Some(state) => match state.provider {
            PinctrlProvider::Loongson2k { .. } => PrerequisiteStatus::RequiresDriver,
            PinctrlProvider::Unsupported => PrerequisiteStatus::UnsupportedProvider,
        },
    };
    Ok(BringUpPlan { controller_mmio : description.controller_mmio,
                     auxiliary_mmio,
                     bus_width : description.bus_width,
                     prerequisites : PrerequisitePlan { clock,
                                                        vmmc:
                                                            supply(description.vmmc_supply),
                                                        vqmmc:
                                                            supply(description.vqmmc_supply),
                                                        pinctrl,
                                                        card_detect },
                     blockers : [ActivationBlocker::DataPathUnavailable,
                                 ActivationBlocker::ExternalDmaExecutorUnavailable,
                                 ActivationBlocker::ClockControlUnavailable,
                                 ActivationBlocker::PowerSequencingUnavailable,
                                 ActivationBlocker::CardDetectUnavailable,
                                 ActivationBlocker::PinControlUnavailable,
                                 ActivationBlocker::InterruptPathUnverified] })
}

const REG_CTL : usize = 0x00;
const REG_PRE : usize = 0x04;
const REG_CARG : usize = 0x08;
const REG_CCTL : usize = 0x0C;
const REG_CSTS : usize = 0x10;
const REG_RSP0 : usize = 0x14;
const REG_RSP1 : usize = 0x18;
const REG_RSP2 : usize = 0x1C;
const REG_RSP3 : usize = 0x20;
const REG_INT : usize = 0x3C;
const REG_DSTS : usize = 0x34;
const REG_IEN : usize = 0x64;

const CTL_ENABLE_CLOCK : u32 = 1 << 0;
const CTL_EXTERNAL_CLOCK : u32 = 1 << 1;
const CTL_RESET : u32 = 1 << 8;
const PRE_ENABLE : u32 = 1 << 31;
const CCTL_HOST : u32 = 1 << 6;
const CCTL_START : u32 = 1 << 8;
const CCTL_WAIT_RESPONSE : u32 = 1 << 9;
const CCTL_LONG_RESPONSE : u32 = 1 << 10;
const INT_COMMAND_SENT : u32 = 1 << 6;
const INT_COMMAND_TIMEOUT : u32 = 1 << 7;
const INT_RESPONSE_CRC : u32 = 1 << 8;
const INT_CLEAR : u32 = 0x3FF;
const CSTS_ON : u32 = 1 << 8;
const DSTS_ACTIVE : u32 = (1 << 0) | (1 << 1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmcIrqAckError {
    UnexpectedSource,
    NoKnownPending,
    UnknownPending(u32),
    Io(MmcError),
}

#[derive(Debug, PartialEq, Eq)]
pub struct MmcIrqAckFailure {
    pub error : MmcIrqAckError,
    pub acknowledged : AcknowledgedIrq,
}

pub trait RegisterIo {
    fn read32(&mut self, offset : usize) -> Result<u32, MmcError>;
    fn write32(&mut self, offset : usize, value : u32) -> Result<(), MmcError>;
}

/// Raw volatile access to the topology-validated MMC controller window.
#[cfg(target_arch = "loongarch64")]
pub struct VolatileRegisters {
    base : *mut u8,
    size : usize,
}

// The diagnostic runtime owns this MMIO window exclusively and the runtime
// slot serializes mutable access. Moving that ownership does not duplicate it.
#[cfg(target_arch = "loongarch64")]
unsafe impl Send for VolatileRegisters {}

#[cfg(target_arch = "loongarch64")]
impl VolatileRegisters {
    /// # Safety
    /// `region` must be mapped device memory and exclusively owned for the
    /// lifetime of this backend. Physical behavior is UNVERIFIED_ON_HARDWARE.
    pub unsafe fn from_region(region : MmioRegion) -> Result<Self, MmcError> {
        if region.base == 0 || region.base % 4 != 0 || region.size < MIN_CONTROLLER_WINDOW {
            return Err(MmcError::RegisterOutOfRange);
        }
        Ok(Self { base : region.base as *mut u8,
                  size : region.size })
    }

    fn register(&self, offset : usize) -> Result<*mut u32, MmcError> {
        if offset % 4 != 0 ||
           offset.checked_add(4)
                 .is_none_or(|end| end > self.size)
        {
            return Err(MmcError::RegisterOutOfRange);
        }
        Ok(unsafe {
            self.base
                .add(offset)
                .cast::<u32>()
        })
    }
}

#[cfg(target_arch = "loongarch64")]
impl RegisterIo for VolatileRegisters {
    fn read32(&mut self, offset : usize) -> Result<u32, MmcError> {
        Ok(unsafe { core::ptr::read_volatile(self.register(offset)?) })
    }

    fn write32(&mut self, offset : usize, value : u32) -> Result<(), MmcError> {
        unsafe { core::ptr::write_volatile(self.register(offset)?, value) };
        Ok(())
    }
}

/// Clear a masked/acknowledged MMC interrupt and produce rearm evidence only
/// when every observed pending bit has documented W1C behavior.
pub fn acknowledge_interrupt<R : RegisterIo>(registers : &mut R,
                                             expected : GlobalIrq,
                                             acknowledged : AcknowledgedIrq)
                                             -> Result<IrqDisposition, MmcIrqAckFailure> {
    if acknowledged.irq() != expected {
        return Err(MmcIrqAckFailure { error : MmcIrqAckError::UnexpectedSource,
                                      acknowledged });
    }
    let status = match registers.read32(REG_INT) {
        Ok(status) => status,
        Err(error) => {
            return Err(MmcIrqAckFailure { error : MmcIrqAckError::Io(error),
                                          acknowledged })
        }
    };
    let known = status & INT_CLEAR;
    if known == 0 {
        return Err(MmcIrqAckFailure { error : MmcIrqAckError::NoKnownPending,
                                      acknowledged });
    }
    if let Err(error) = registers.write32(REG_INT, known) {
        return Err(MmcIrqAckFailure { error : MmcIrqAckError::Io(error),
                                      acknowledged });
    }
    let unknown = status & !INT_CLEAR;
    if unknown != 0 {
        return Err(MmcIrqAckFailure { error : MmcIrqAckError::UnknownPending(unknown),
                                      acknowledged });
    }
    Ok(IrqDisposition::Rearm(DeviceAckedIrq::after_device_clear(expected)))
}

pub struct Uninitialized;
pub struct Preflighted;
pub struct ClockConfigured;
pub struct CommandReady;
pub struct CommandRecoveryRequired;

pub struct Host<R, S = Uninitialized> {
    registers : R,
    poll_limit : usize,
    _state : PhantomData<S>,
}

impl<R, S> Host<R, S> {
    fn change_state<T>(self) -> Host<R, T> {
        Host { registers : self.registers,
               poll_limit : self.poll_limit,
               _state : PhantomData }
    }

    #[cfg(test)]
    fn into_inner(self) -> R { self.registers }
}

pub trait ResetDelay {
    /// Wait at least the requested duration before returning.
    fn delay_milliseconds(&mut self, milliseconds : u32);
}

/// Explicit acceptance of the upstream-derived reset sequence on a board.
pub struct HostPreflightAuthority {
    _private : (),
}

impl HostPreflightAuthority {
    /// # Safety
    /// The caller must verify exclusive controller ownership, reset semantics,
    /// delay availability, W1C interrupt behavior and the recovery procedure.
    pub const unsafe fn assume_board_verified() -> Self { Self { _private : () } }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostPreflightStage {
    WriteReset,
    WriteExternalClock,
    ReadbackControl,
    ControlMismatch,
    ClearInterrupts,
    EnableInterrupts,
    ReadbackInterruptEnable,
    InterruptEnableMismatch,
    ReadCommandStatus,
    ReadDataStatus,
    IdleTimeout,
}

pub struct HostPreflightFailure<R> {
    pub stage : HostPreflightStage,
    pub error : Option<MmcError>,
    pub observed_control : Option<u32>,
    pub observed_interrupt_enable : Option<u32>,
    pub observed_command_status : Option<u32>,
    pub observed_data_status : Option<u32>,
    host : Host<R, Uninitialized>,
}

impl<R> core::fmt::Debug for HostPreflightFailure<R> {
    fn fmt(&self, formatter : &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.debug_struct("HostPreflightFailure")
                 .field("stage", &self.stage)
                 .field("error", &self.error)
                 .field("observed_control",
                        &self.observed_control)
                 .field("observed_interrupt_enable",
                        &self.observed_interrupt_enable)
                 .field("observed_command_status",
                        &self.observed_command_status)
                 .field("observed_data_status",
                        &self.observed_data_status)
                 .finish_non_exhaustive()
    }
}

impl<R : RegisterIo> HostPreflightFailure<R> {
    /// Retry the entire reset sequence. A failed write has unknown effect, so
    /// retry starts from RESET rather than continuing at the failed stage.
    pub fn retry(self,
                 delay : &mut impl ResetDelay,
                 authority : &HostPreflightAuthority)
                 -> Result<Host<R, Preflighted>, Self> {
        self.host
            .preflight(delay, authority)
    }

    pub fn into_host(self) -> Host<R, Uninitialized> { self.host }
}

impl<R : RegisterIo> Host<R, Uninitialized> {
    pub fn new(registers : R, poll_limit : usize) -> Self {
        Self { registers,
               poll_limit : poll_limit.max(1),
               _state : PhantomData }
    }

    /// Apply the upstream power-up reset sequence and prove command/data idle.
    ///
    /// Physical behavior remains `UNVERIFIED_ON_HARDWARE`. The 10 ms delay is
    /// upstream-derived; no reset self-clear behavior is assumed.
    pub fn preflight(mut self,
                     delay : &mut impl ResetDelay,
                     _authority : &HostPreflightAuthority)
                     -> Result<Host<R, Preflighted>, HostPreflightFailure<R>> {
        if let Err(error) = self.registers
                                .write32(REG_CTL, CTL_RESET)
        {
            return Err(self.failure(HostPreflightStage::WriteReset,
                                    Some(error),
                                    None,
                                    None,
                                    None,
                                    None));
        }
        delay.delay_milliseconds(10);
        if let Err(error) = self.registers
                                .write32(REG_CTL, CTL_EXTERNAL_CLOCK)
        {
            return Err(self.failure(HostPreflightStage::WriteExternalClock,
                                    Some(error),
                                    None,
                                    None,
                                    None,
                                    None));
        }
        let control = match self.registers
                                .read32(REG_CTL)
        {
            Ok(value) => value,
            Err(error) => {
                return Err(self.failure(HostPreflightStage::ReadbackControl,
                                        Some(error),
                                        None,
                                        None,
                                        None,
                                        None))
            }
        };
        if control != CTL_EXTERNAL_CLOCK {
            return Err(self.failure(HostPreflightStage::ControlMismatch,
                                    None,
                                    Some(control),
                                    None,
                                    None,
                                    None));
        }
        if let Err(error) = self.registers
                                .write32(REG_INT, INT_CLEAR)
        {
            return Err(self.failure(HostPreflightStage::ClearInterrupts,
                                    Some(error),
                                    Some(control),
                                    None,
                                    None,
                                    None));
        }
        if let Err(error) = self.registers
                                .write32(REG_IEN, INT_CLEAR)
        {
            return Err(self.failure(HostPreflightStage::EnableInterrupts,
                                    Some(error),
                                    Some(control),
                                    None,
                                    None,
                                    None));
        }
        let interrupt_enable = match self.registers
                                         .read32(REG_IEN)
        {
            Ok(value) => value,
            Err(error) => {
                return Err(self.failure(HostPreflightStage::ReadbackInterruptEnable,
                                        Some(error),
                                        Some(control),
                                        None,
                                        None,
                                        None))
            }
        };
        if interrupt_enable != INT_CLEAR {
            return Err(self.failure(HostPreflightStage::InterruptEnableMismatch,
                                    None,
                                    Some(control),
                                    Some(interrupt_enable),
                                    None,
                                    None));
        }

        let mut command_status = None;
        let mut data_status = None;
        for _ in 0..self.poll_limit {
            let command = match self.registers
                                    .read32(REG_CSTS)
            {
                Ok(value) => value,
                Err(error) => {
                    return Err(self.failure(HostPreflightStage::ReadCommandStatus,
                                            Some(error),
                                            Some(control),
                                            Some(interrupt_enable),
                                            None,
                                            data_status))
                }
            };
            command_status = Some(command);
            let data = match self.registers
                                 .read32(REG_DSTS)
            {
                Ok(value) => value,
                Err(error) => {
                    return Err(self.failure(HostPreflightStage::ReadDataStatus,
                                            Some(error),
                                            Some(control),
                                            Some(interrupt_enable),
                                            command_status,
                                            None))
                }
            };
            data_status = Some(data);
            if command & CSTS_ON == 0 && data & DSTS_ACTIVE == 0 {
                return Ok(self.change_state());
            }
        }
        Err(self.failure(HostPreflightStage::IdleTimeout,
                         None,
                         Some(control),
                         Some(interrupt_enable),
                         command_status,
                         data_status))
    }

    fn failure(self,
               stage : HostPreflightStage,
               error : Option<MmcError>,
               observed_control : Option<u32>,
               observed_interrupt_enable : Option<u32>,
               observed_command_status : Option<u32>,
               observed_data_status : Option<u32>)
               -> HostPreflightFailure<R> {
        HostPreflightFailure { stage,
                               error,
                               observed_control,
                               observed_interrupt_enable,
                               observed_command_status,
                               observed_data_status,
                               host : self }
    }
}

/// Match the upstream driver's integer prescaler policy: round upward and clamp
/// at 255. If the requested divisor exceeds 255, the returned actual clock can
/// exceed the target; callers must inspect it before touching hardware.
pub fn clock_prescaler(input_hz : u32, target_hz : u32) -> Result<(u8, u32), MmcError> {
    if input_hz == 0 || target_hz == 0 {
        return Err(MmcError::InvalidParameter);
    }
    let divider = input_hz.div_ceil(target_hz)
                          .clamp(1, 255);
    Ok((divider as u8, input_hz / divider))
}

/// Opaque controller-private clock plan derived from coherent parent evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControllerClockPlan {
    parent_hz : u32,
    requested_hz : u32,
    divider : u8,
    actual_hz : u32,
}

impl ControllerClockPlan {
    pub fn from_parent(parent : ConsistentClockSnapshot,
                       requested_hz : u32)
                       -> Result<Self, MmcError> {
        let parent_hz = u32::try_from(parent.snapshot()
                                            .apb_hz).map_err(|_| MmcError::InvalidParameter)?;
        let (divider, actual_hz) = clock_prescaler(parent_hz, requested_hz)?;
        Ok(Self { parent_hz,
                  requested_hz,
                  divider,
                  actual_hz })
    }

    pub const fn parent_hz(&self) -> u32 { self.parent_hz }
    pub const fn requested_hz(&self) -> u32 { self.requested_hz }
    pub const fn divider(&self) -> u8 { self.divider }
    pub const fn actual_hz(&self) -> u32 { self.actual_hz }
    const fn pre_value(&self) -> u32 { PRE_ENABLE | self.divider as u32 }
}

/// Readback-verified controller-private clock state.
///
/// Physical clock output remains `UNVERIFIED_ON_HARDWARE`; this token is not
/// command, DMA or aggregate prerequisite authority.
#[derive(Debug)]
pub struct ControllerClockReady {
    plan : ControllerClockPlan,
}

impl ControllerClockReady {
    pub const fn plan(&self) -> ControllerClockPlan { self.plan }
}

/// Explicit acceptance of an unverified physical MMC clock write.
pub struct ControllerClockAuthority {
    _private : (),
}

impl ControllerClockAuthority {
    /// # Safety
    /// The caller must verify the target board, exclusive controller ownership,
    /// parent-clock stability, register semantics and recovery procedure.
    pub const unsafe fn assume_board_verified() -> Self { Self { _private : () } }
}

struct ClockTransactionGate {
    busy : AtomicBool,
}

impl ClockTransactionGate {
    const fn new() -> Self { Self { busy : AtomicBool::new(false) } }

    fn try_enter(&self) -> Result<ClockTransactionGuard<'_>, ClockTransactionBusy> {
        self.busy
            .compare_exchange(false,
                              true,
                              Ordering::AcqRel,
                              Ordering::Acquire)
            .map(|_| ClockTransactionGuard { gate : self })
            .map_err(|_| ClockTransactionBusy)
    }
}

static CLOCK_TRANSACTION_GATE : ClockTransactionGate = ClockTransactionGate::new();

pub fn try_begin_clock_transaction(
    )
    -> Result<ClockTransactionGuard<'static>, ClockTransactionBusy>
{
    CLOCK_TRANSACTION_GATE.try_enter()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockTransactionBusy;

pub struct ClockTransactionGuard<'a> {
    gate : &'a ClockTransactionGate,
}

impl Drop for ClockTransactionGuard<'_> {
    fn drop(&mut self) {
        self.gate
            .busy
            .store(false, Ordering::Release);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControllerClockStage {
    ObservePre,
    ObserveControl,
    ObserveMismatch,
    PreflightPre,
    PreflightControl,
    WritePre,
    ReadbackPre,
    ReadbackPreMismatch,
    WriteControl,
    ReadbackControl,
    ReadbackControlMismatch,
    RevalidatePre,
    RevalidateControl,
    RevalidateMismatch,
}

/// Verify an already-configured controller clock without writing registers.
pub fn observe_controller_clock(registers : &mut impl RegisterIo,
                                plan : ControllerClockPlan)
                                -> Result<ControllerClockReady, ControllerClockRecovery> {
    let pre = registers.read32(REG_PRE)
                       .map_err(|error| {
                           ControllerClockRecovery { stage : ControllerClockStage::ObservePre,
                                                     plan,
                                                     observed_pre : None,
                                                     observed_control : None,
                                                     error : Some(error) }
                       })?;
    let control = registers.read32(REG_CTL)
                           .map_err(|error| {
                               ControllerClockRecovery { stage:
                                                             ControllerClockStage::ObserveControl,
                                                         plan,
                                                         observed_pre : Some(pre),
                                                         observed_control : None,
                                                         error : Some(error) }
                           })?;
    verify_controller_clock(plan, pre, control).map_err(|()| {
                                                   ControllerClockRecovery {
                                                      stage : ControllerClockStage::ObserveMismatch,
                                                      plan,
                                                      observed_pre : Some(pre),
                                                      observed_control : Some(control),
                                                      error : None,
                                                  }
                                               })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControllerClockRecovery {
    pub stage : ControllerClockStage,
    pub plan : ControllerClockPlan,
    pub observed_pre : Option<u32>,
    pub observed_control : Option<u32>,
    pub error : Option<MmcError>,
}

impl ControllerClockRecovery {
    /// Re-read controller state after an uncertain transaction; never writes.
    pub fn revalidate(&self,
                      registers : &mut impl RegisterIo,
                      _guard : &mut ClockTransactionGuard<'_>)
                      -> Result<ControllerClockReady, Self> {
        let pre = registers.read32(REG_PRE)
                           .map_err(|error| Self { stage : ControllerClockStage::RevalidatePre,
                                                   plan : self.plan,
                                                   observed_pre : None,
                                                   observed_control : None,
                                                   error : Some(error) })?;
        let control =
            registers.read32(REG_CTL)
                     .map_err(|error| Self { stage : ControllerClockStage::RevalidateControl,
                                             plan : self.plan,
                                             observed_pre : Some(pre),
                                             observed_control : None,
                                             error : Some(error) })?;
        verify_controller_clock(self.plan, pre, control).map_err(|()| Self {
                                                               stage : ControllerClockStage::RevalidateMismatch,
                                                               plan : self.plan,
                                                               observed_pre : Some(pre),
                                                               observed_control : Some(control),
                                                               error : None,
                                                           })
    }
}

fn verify_controller_clock(plan : ControllerClockPlan,
                           pre : u32,
                           control : u32)
                           -> Result<ControllerClockReady, ()> {
    if pre == plan.pre_value() && control & CTL_ENABLE_CLOCK != 0 {
        Ok(ControllerClockReady { plan })
    } else {
        Err(())
    }
}

/// Fresh-read, conditionally program and read back the MMC-private clock.
pub fn apply_controller_clock(registers : &mut impl RegisterIo,
                              plan : ControllerClockPlan,
                              _authority : &ControllerClockAuthority,
                              _guard : &mut ClockTransactionGuard<'_>)
                              -> Result<ControllerClockReady, ControllerClockRecovery> {
    let mut pre =
        registers.read32(REG_PRE)
                 .map_err(|error| ControllerClockRecovery { stage:
                                                                ControllerClockStage::PreflightPre,
                                                            plan,
                                                            observed_pre : None,
                                                            observed_control : None,
                                                            error : Some(error) })?;
    let mut control =
        registers.read32(REG_CTL)
                 .map_err(|error| {
                     ControllerClockRecovery { stage : ControllerClockStage::PreflightControl,
                                               plan,
                                               observed_pre : Some(pre),
                                               observed_control : None,
                                               error : Some(error) }
                 })?;
    if verify_controller_clock(plan, pre, control).is_ok() {
        return Ok(ControllerClockReady { plan });
    }

    if pre != plan.pre_value() {
        registers.write32(REG_PRE, plan.pre_value())
                 .map_err(|error| ControllerClockRecovery { stage:
                                                                ControllerClockStage::WritePre,
                                                            plan,
                                                            observed_pre : None,
                                                            observed_control : Some(control),
                                                            error : Some(error) })?;
        pre = registers.read32(REG_PRE)
                       .map_err(|error| {
                           ControllerClockRecovery { stage : ControllerClockStage::ReadbackPre,
                                                     plan,
                                                     observed_pre : None,
                                                     observed_control : Some(control),
                                                     error : Some(error) }
                       })?;
        if pre != plan.pre_value() {
            return Err(ControllerClockRecovery { stage:
                                                     ControllerClockStage::ReadbackPreMismatch,
                                                 plan,
                                                 observed_pre : Some(pre),
                                                 observed_control : Some(control),
                                                 error : None });
        }
    }

    if control & CTL_ENABLE_CLOCK == 0 {
        let desired = control | CTL_ENABLE_CLOCK;
        registers.write32(REG_CTL, desired)
                 .map_err(|error| ControllerClockRecovery { stage:
                                                                ControllerClockStage::WriteControl,
                                                            plan,
                                                            observed_pre : Some(pre),
                                                            observed_control : None,
                                                            error : Some(error) })?;
        control = registers.read32(REG_CTL)
                           .map_err(|error| {
                               ControllerClockRecovery { stage:
                                                             ControllerClockStage::ReadbackControl,
                                                         plan,
                                                         observed_pre : Some(pre),
                                                         observed_control : None,
                                                         error : Some(error) }
                           })?;
        if control != desired {
            return Err(ControllerClockRecovery { stage:
                                                     ControllerClockStage::ReadbackControlMismatch,
                                                 plan,
                                                 observed_pre : Some(pre),
                                                 observed_control : Some(control),
                                                 error : None });
        }
    }

    verify_controller_clock(plan, pre, control).map_err(|()| {
                                                   ControllerClockRecovery { stage:
                                                ControllerClockStage::ReadbackControlMismatch,
                                            plan,
                                            observed_pre : Some(pre),
                                            observed_control : Some(control),
                                            error : None }
                                               })
}

pub struct HostClockFailure<R> {
    pub recovery : ControllerClockRecovery,
    host : Host<R, Preflighted>,
}

impl<R> core::fmt::Debug for HostClockFailure<R> {
    fn fmt(&self, formatter : &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.debug_struct("HostClockFailure")
                 .field("recovery", &self.recovery)
                 .finish_non_exhaustive()
    }
}

impl<R : RegisterIo> HostClockFailure<R> {
    pub fn retry(self,
                 authority : &ControllerClockAuthority,
                 guard : &mut ClockTransactionGuard<'_>)
                 -> Result<(Host<R, ClockConfigured>, ControllerClockReady), Self> {
        self.host
            .configure_controller_clock(self.recovery.plan, authority, guard)
    }

    pub fn into_host(self) -> Host<R, Preflighted> { self.host }
}

impl<R : RegisterIo> Host<R, Preflighted> {
    pub fn configure_controller_clock(
        mut self,
        plan : ControllerClockPlan,
        authority : &ControllerClockAuthority,
        guard : &mut ClockTransactionGuard<'_>)
        -> Result<(Host<R, ClockConfigured>, ControllerClockReady), HostClockFailure<R>> {
        match apply_controller_clock(&mut self.registers,
                                     plan,
                                     authority,
                                     guard)
        {
            Ok(ready) => Ok((self.change_state(), ready)),
            Err(recovery) => Err(HostClockFailure { recovery,
                                                    host : self }),
        }
    }
}

impl<R> Host<R, ClockConfigured> {
    /// Consume the complete, post-reset prerequisite proof exactly once.
    pub fn authorize(self, _proof : ControllerPrerequisiteProof) -> Host<R, CommandReady> {
        self.change_state()
    }

    #[cfg(test)]
    fn authorize_host_fixture(self) -> Host<R, CommandReady> { self.change_state() }
}

/// Response width promised by a non-data MMC command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseType {
    None,
    Short,
    Long,
}

/// Protocol-level checks requested by the command. Only `Unchecked` is
/// currently supported because upstream does not establish CHECK/BUSYEND.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseValidation {
    Unchecked,
    Crc,
    CrcAndBusy,
}

/// Transfer intent used to reject the unimplemented data path before MMIO.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandTransfer {
    None,
    /// Reserved for a future PIO/APBDMA contract; currently rejected.
    Data,
}

/// Failure to construct a command that the current Host can execute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandDescriptorError {
    InvalidIndex,
    DataUnsupported,
    ResponsePolicyUnsupported,
}

/// A prevalidated command contract. Invalid and data-bearing commands cannot
/// reach the Host MMIO method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandDescriptor {
    index : u8,
    argument : u32,
    response : ResponseType,
}

impl CommandDescriptor {
    /// Validate the command index and reject all data-bearing or unsupported
    /// response-policy requests before the Host can perform MMIO.
    pub fn new(index : u8,
               argument : u32,
               response : ResponseType,
               validation : ResponseValidation,
               transfer : CommandTransfer)
               -> Result<Self, CommandDescriptorError> {
        if index > 63 {
            return Err(CommandDescriptorError::InvalidIndex);
        }
        if transfer != CommandTransfer::None {
            return Err(CommandDescriptorError::DataUnsupported);
        }
        if validation != ResponseValidation::Unchecked {
            return Err(CommandDescriptorError::ResponsePolicyUnsupported);
        }
        Ok(Self { index,
                  argument,
                  response })
    }
}

/// Response words read according to the validated descriptor contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandResponse {
    None,
    Short(u32),
    /// RSP0 through RSP3 in register-offset order. Physical protocol word
    /// mapping remains UNVERIFIED_ON_HARDWARE.
    Long([u32; 4]),
}

impl<R : RegisterIo> Host<R, CommandReady> {
    /// Execute a non-data command by bounded polling.
    ///
    /// Register definitions and W1C interrupt behavior follow the upstream
    /// Linux driver. Physical response ordering remains UNVERIFIED_ON_HARDWARE.
    pub fn execute_command(mut self, command : CommandDescriptor) -> CommandOutcome<R> {
        if let Err(error) = self.registers
                                .write32(REG_INT, INT_CLEAR)
        {
            return self.command_failure(CommandStage::ClearInterrupts, error);
        }
        if let Err(error) = self.registers
                                .write32(REG_CARG, command.argument)
        {
            return self.command_failure(CommandStage::WriteArgument, error);
        }
        let mut control = command.index as u32 | CCTL_HOST | CCTL_START;
        match command.response {
            ResponseType::None => {}
            ResponseType::Short => control |= CCTL_WAIT_RESPONSE,
            ResponseType::Long => {
                control |= CCTL_WAIT_RESPONSE | CCTL_LONG_RESPONSE
            }
        }
        if let Err(error) = self.registers
                                .write32(REG_CCTL, control)
        {
            return self.command_failure(CommandStage::StartCommand, error);
        }
        for _ in 0..self.poll_limit {
            let interrupts = match self.registers
                                       .read32(REG_INT)
            {
                Ok(interrupts) => interrupts,
                Err(error) => return self.command_failure(CommandStage::PollInterrupts, error),
            };
            if interrupts & INT_COMMAND_TIMEOUT != 0 {
                return self.command_failure(CommandStage::CommandTimeout,
                                            MmcError::ResponseTimeout);
            }
            if interrupts & INT_RESPONSE_CRC != 0 {
                return self.command_failure(CommandStage::ResponseCrc, MmcError::ResponseCrc);
            }
            if interrupts & INT_COMMAND_SENT != 0 {
                if let Err(error) = self.registers
                                        .write32(REG_INT, interrupts & INT_CLEAR)
                {
                    return self.command_failure(CommandStage::AcknowledgeCompletion, error);
                }
                let response = match command.response {
                    ResponseType::None => CommandResponse::None,
                    ResponseType::Short => {
                        match self.registers
                                  .read32(REG_RSP0)
                        {
                            Ok(value) => CommandResponse::Short(value),
                            Err(error) => {
                                return self.command_failure(CommandStage::ReadResponse0, error)
                            }
                        }
                    }
                    ResponseType::Long => {
                        let mut response = [0; 4];
                        for (word, (offset, stage)) in
                            response.iter_mut()
                                    .zip([(REG_RSP0, CommandStage::ReadResponse0),
                                          (REG_RSP1, CommandStage::ReadResponse1),
                                          (REG_RSP2, CommandStage::ReadResponse2),
                                          (REG_RSP3, CommandStage::ReadResponse3)])
                        {
                            *word = match self.registers
                                              .read32(offset)
                            {
                                Ok(value) => value,
                                Err(error) => return self.command_failure(stage, error),
                            };
                        }
                        CommandResponse::Long(response)
                    }
                };
                if let Err(error) = self.registers
                                        .write32(REG_CARG, 0)
                {
                    return self.command_failure(CommandStage::CleanupArgument, error);
                }
                if let Err(error) = self.registers
                                        .write32(REG_CCTL, 0)
                {
                    return self.command_failure(CommandStage::CleanupControl, error);
                }
                return CommandOutcome::Completed { host : self,
                                                   response };
            }
        }
        self.command_failure(CommandStage::PollTimeout, MmcError::Timeout)
    }

    fn command_failure(self, stage : CommandStage, error : MmcError) -> CommandOutcome<R> {
        CommandOutcome::RecoveryRequired(CommandRecovery { stage,
                                                           error,
                                                           origin_stage : stage,
                                                           origin_error : error,
                                                           observed_command_status : None,
                                                           observed_data_status : None,
                                                           observed_interrupts : None,
                                                           observed_argument : None,
                                                           observed_control : None,
                                                           host : self.change_state() })
    }
}

pub enum CommandOutcome<R> {
    Completed {
        host : Host<R, CommandReady>,
        response : CommandResponse,
    },
    RecoveryRequired(CommandRecovery<R>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandStage {
    ClearInterrupts,
    WriteArgument,
    StartCommand,
    PollInterrupts,
    CommandTimeout,
    ResponseCrc,
    PollTimeout,
    AcknowledgeCompletion,
    ReadResponse0,
    ReadResponse1,
    ReadResponse2,
    ReadResponse3,
    CleanupArgument,
    CleanupControl,
    RevalidateCommandStatus,
    RevalidateDataStatus,
    RevalidateInterrupts,
    RevalidateBusy,
    RevalidateUnknownInterrupt,
    RevalidateClearInterrupts,
    RevalidateInterruptReadback,
    RevalidateInterruptStillPending,
    RevalidateCleanupArgument,
    RevalidateCleanupControl,
    RevalidateArgumentReadback,
    RevalidateArgumentMismatch,
    RevalidateControlReadback,
    RevalidateControlMismatch,
}

pub struct CommandRecovery<R> {
    /// Immutable cause that first removed command-ready ownership.
    pub origin_stage : CommandStage,
    pub origin_error : MmcError,
    /// Current recovery/revalidation stage and most recent error.
    pub stage : CommandStage,
    pub error : MmcError,
    pub observed_command_status : Option<u32>,
    pub observed_data_status : Option<u32>,
    pub observed_interrupts : Option<u32>,
    pub observed_argument : Option<u32>,
    pub observed_control : Option<u32>,
    host : Host<R, CommandRecoveryRequired>,
}

impl<R> core::fmt::Debug for CommandRecovery<R> {
    fn fmt(&self, formatter : &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.debug_struct("CommandRecovery")
                 .field("origin_stage", &self.origin_stage)
                 .field("origin_error", &self.origin_error)
                 .field("stage", &self.stage)
                 .field("error", &self.error)
                 .field("observed_command_status",
                        &self.observed_command_status)
                 .field("observed_data_status",
                        &self.observed_data_status)
                 .field("observed_interrupts",
                        &self.observed_interrupts)
                 .field("observed_argument",
                        &self.observed_argument)
                 .field("observed_control",
                        &self.observed_control)
                 .finish_non_exhaustive()
    }
}

impl<R : RegisterIo> CommandRecovery<R> {
    /// Prove that the controller returned idle and all documented W1C status
    /// was cleared before making the same authorized session command-capable.
    /// Physical recovery behavior remains UNVERIFIED_ON_HARDWARE.
    pub fn revalidate(mut self) -> Result<Host<R, CommandReady>, Self> {
        let command = match self.host.registers
                                     .read32(REG_CSTS)
        {
            Ok(value) => value,
            Err(error) => return Err(self.at(CommandStage::RevalidateCommandStatus, error)),
        };
        self.observed_command_status = Some(command);
        let data = match self.host.registers
                                  .read32(REG_DSTS)
        {
            Ok(value) => value,
            Err(error) => return Err(self.at(CommandStage::RevalidateDataStatus, error)),
        };
        self.observed_data_status = Some(data);
        if command & CSTS_ON != 0 || data & DSTS_ACTIVE != 0 {
            return Err(self.at(CommandStage::RevalidateBusy, MmcError::Timeout));
        }
        let interrupts = match self.host.registers
                                        .read32(REG_INT)
        {
            Ok(value) => value,
            Err(error) => return Err(self.at(CommandStage::RevalidateInterrupts, error)),
        };
        self.observed_interrupts = Some(interrupts);
        if interrupts & !INT_CLEAR != 0 {
            return Err(self.at(CommandStage::RevalidateUnknownInterrupt,
                               MmcError::InvalidParameter));
        }
        if interrupts != 0 {
            if let Err(error) = self.host.registers
                                         .write32(REG_INT, interrupts)
            {
                return Err(self.at(CommandStage::RevalidateClearInterrupts, error));
            }
            let readback = match self.host.registers
                                     .read32(REG_INT)
            {
                Ok(value) => value,
                Err(error) => {
                    return Err(self.at(CommandStage::RevalidateInterruptReadback, error))
                }
            };
            self.observed_interrupts = Some(readback);
            if readback != 0 {
                return Err(self.at(CommandStage::RevalidateInterruptStillPending,
                                   MmcError::Timeout));
            }
        }
        if let Err(error) = self.host.registers
                                     .write32(REG_CARG, 0)
        {
            return Err(self.at(CommandStage::RevalidateCleanupArgument, error));
        }
        if let Err(error) = self.host.registers
                                     .write32(REG_CCTL, 0)
        {
            return Err(self.at(CommandStage::RevalidateCleanupControl, error));
        }
        let argument = match self.host.registers
                                 .read32(REG_CARG)
        {
            Ok(value) => value,
            Err(error) => return Err(self.at(CommandStage::RevalidateArgumentReadback, error)),
        };
        self.observed_argument = Some(argument);
        if argument != 0 {
            return Err(self.at(CommandStage::RevalidateArgumentMismatch,
                               MmcError::InvalidParameter));
        }
        let control = match self.host.registers
                                .read32(REG_CCTL)
        {
            Ok(value) => value,
            Err(error) => return Err(self.at(CommandStage::RevalidateControlReadback, error)),
        };
        self.observed_control = Some(control);
        if control != 0 {
            return Err(self.at(CommandStage::RevalidateControlMismatch,
                               MmcError::InvalidParameter));
        }
        Ok(self.host.change_state())
    }

    /// Discard the previous prerequisite proof and require full reset,
    /// controller-clock configuration and authorization again.
    pub fn into_uninitialized(self) -> Host<R, Uninitialized> { self.host.change_state() }

    fn at(mut self, stage : CommandStage, error : MmcError) -> Self {
        self.stage = stage;
        self.error = error;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock;
    use crate::topology::{
        CardDetect, FixedSupplyControl, InterruptSpec, MmcClockProvider, NamedResource,
        ResourceSpecifier, SupplyDescription, SupplyProvider,
    };
    use alloc::vec;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum AckEvent {
        Read(usize),
        Write(usize, u32),
    }

    struct AckRegisters {
        status : u32,
        fail_read : bool,
        fail_write : bool,
        events : alloc::vec::Vec<AckEvent>,
    }

    impl RegisterIo for AckRegisters {
        fn read32(&mut self, offset : usize) -> Result<u32, MmcError> {
            self.events
                .push(AckEvent::Read(offset));
            if self.fail_read {
                Err(MmcError::RegisterOutOfRange)
            } else {
                Ok(self.status)
            }
        }
        fn write32(&mut self, offset : usize, value : u32) -> Result<(), MmcError> {
            self.events
                .push(AckEvent::Write(offset, value));
            if self.fail_write {
                Err(MmcError::RegisterOutOfRange)
            } else {
                Ok(())
            }
        }
    }

    fn ack_registers(status : u32) -> AckRegisters {
        AckRegisters { status,
                       fail_read : false,
                       fail_write : false,
                       events : alloc::vec::Vec::new() }
    }

    fn acknowledged(irq : GlobalIrq) -> AcknowledgedIrq { AcknowledgedIrq::after_mask_ack(irq) }

    fn description() -> MmcDescription {
        MmcDescription { controller_mmio : MmioRegion { base : 0x1FE2_C000,
                                                        size : 0x68 },
                         auxiliary_mmio : Some(MmioRegion { base : 0x1FE0_0438,
                                                            size : 8 }),
                         interrupt : InterruptSpec { parent_phandle : 1,
                                                     cells : [31, 4, 0, 0],
                                                     cell_count : 2 },
                         clocks : vec![NamedResource {
                name : None,
                specifier : ResourceSpecifier { provider_phandle : 2, args : vec![0] },
            }],
                         clock_provider:
                             MmcClockProvider::Loongson2k { mmio : MmioRegion { base:
                                                                                    0x1FE0_0480,
                                                                                size : 0x58 },
                                                            reference_hz : 100_000_000 },
                         dma : None,
                         bus_width : 4,
                         pinctrl : None,
                         card_detect : CardDetect::NonRemovable,
                         vmmc_supply : None,
                         vqmmc_supply : None }
    }

    #[test]
    fn preserves_dma_routing_window_but_refuses_activation() {
        let plan = plan(&description()).unwrap();
        assert_eq!(plan.controller_mmio
                       .size,
                   0x68);
        assert_eq!(plan.auxiliary_mmio
                       .base,
                   0x1FE0_0438);
        assert_eq!(plan.bus_width, 4);
        assert!(!plan.can_activate());
        assert!(plan.blockers
                    .contains(&ActivationBlocker::DataPathUnavailable));
        assert_eq!(plan.prerequisites
                       .clock,
                   PrerequisiteStatus::RequiresDriver);
        assert_eq!(plan.prerequisites
                       .card_detect,
                   PrerequisiteStatus::ReadyByTopology);
        assert_eq!(plan.prerequisites
                       .vmmc,
                   PrerequisiteStatus::ImplicitBoardSupply);
        assert_eq!(plan.prerequisites
                       .pinctrl,
                   PrerequisiteStatus::Missing);
    }

    #[test]
    fn classifies_pinctrl_without_assuming_firmware_selected_the_state() {
        let mut value = description();
        value.pinctrl = Some(crate::topology::MmcPinctrlDescription {
            state_phandle : 5,
            provider : PinctrlProvider::Loongson2k {
                mmio : MmioRegion { base : 0x1fe0_0420, size : 0x18 },
            },
        });
        assert_eq!(plan(&value).unwrap()
                               .prerequisites
                               .pinctrl,
                   PrerequisiteStatus::RequiresDriver);
        value.pinctrl
             .as_mut()
             .unwrap()
             .provider = PinctrlProvider::Unsupported;
        assert_eq!(plan(&value).unwrap()
                               .prerequisites
                               .pinctrl,
                   PrerequisiteStatus::UnsupportedProvider);
    }

    #[test]
    fn classifies_power_readiness_without_assuming_fixed_regulators_are_enabled() {
        let mut value = description();
        value.vmmc_supply =
            Some(SupplyDescription { phandle : 3,
                                     provider : SupplyProvider::Fixed { control:
                                                                            FixedSupplyControl::None,
                                                                        always_on : false,
                                                                        boot_on : false } });
        value.vqmmc_supply =
            Some(SupplyDescription { phandle : 4,
                                     provider : SupplyProvider::Fixed { control:
                                                                            FixedSupplyControl::None,
                                                                        always_on : true,
                                                                        boot_on : false } });
        let readiness = plan(&value).unwrap();
        assert_eq!(readiness.prerequisites
                            .vmmc,
                   PrerequisiteStatus::ReadyByTopology);
        assert_eq!(readiness.prerequisites
                            .vqmmc,
                   PrerequisiteStatus::ReadyByTopology);

        value.vmmc_supply
             .as_mut()
             .unwrap()
             .provider = SupplyProvider::Fixed { control : FixedSupplyControl::Gpio,
                                                 always_on : true,
                                                 boot_on : true };
        assert_eq!(plan(&value).unwrap()
                               .prerequisites
                               .vmmc,
                   PrerequisiteStatus::RequiresDriver);

        value.vmmc_supply
             .as_mut()
             .unwrap()
             .provider = SupplyProvider::Unsupported;
        assert_eq!(plan(&value).unwrap()
                               .prerequisites
                               .vmmc,
                   PrerequisiteStatus::UnsupportedProvider);
    }

    #[test]
    fn rejects_resources_that_cannot_back_future_register_io() {
        let mut value = description();
        value.auxiliary_mmio = None;
        assert_eq!(plan(&value),
                   Err(PlanError::MissingAuxiliaryWindow));

        let mut value = description();
        value.controller_mmio
             .size = 0x64;
        assert_eq!(plan(&value),
                   Err(PlanError::ControllerWindowTooSmall));

        let mut value = description();
        value.clocks.clear();
        assert_eq!(plan(&value),
                   Err(PlanError::MissingClock));
    }

    struct MockRegisters {
        values : [u32; 26],
        interrupts : u32,
    }

    struct CommandRegisters {
        values : [u32; 26],
        interrupts : u32,
        next_interrupts : u32,
        command_status : u32,
        data_status : u32,
        reads : usize,
        writes : usize,
        response_reads : alloc::vec::Vec<usize>,
        started_control : Option<u32>,
        fail_read : Option<usize>,
        fail_write : Option<usize>,
        ignore_argument_zero : bool,
        ignore_control_zero : bool,
    }

    impl RegisterIo for CommandRegisters {
        fn read32(&mut self, offset : usize) -> Result<u32, MmcError> {
            self.reads += 1;
            if self.fail_read == Some(self.reads) {
                return Err(MmcError::RegisterOutOfRange);
            }
            if matches!(offset, REG_RSP0 | REG_RSP1 | REG_RSP2 | REG_RSP3) {
                self.response_reads
                    .push(offset);
            }
            match offset {
                REG_INT => Ok(self.interrupts),
                REG_CSTS => Ok(self.command_status),
                REG_DSTS => Ok(self.data_status),
                _ => self.values
                         .get(offset / 4)
                         .copied()
                         .ok_or(MmcError::RegisterOutOfRange),
            }
        }

        fn write32(&mut self, offset : usize, value : u32) -> Result<(), MmcError> {
            self.writes += 1;
            if self.fail_write == Some(self.writes) {
                return Err(MmcError::RegisterOutOfRange);
            }
            match offset {
                REG_INT => self.interrupts &= !value,
                REG_CCTL => {
                    if value == 0 && self.ignore_control_zero {
                        return Ok(());
                    }
                    self.values[REG_CCTL / 4] = value;
                    if value & CCTL_START != 0 {
                        self.started_control = Some(value);
                        self.interrupts = self.next_interrupts;
                    }
                }
                _ => {
                    if offset == REG_CARG && value == 0 && self.ignore_argument_zero {
                        return Ok(());
                    }
                    *self.values
                         .get_mut(offset / 4)
                         .ok_or(MmcError::RegisterOutOfRange)? = value;
                }
            }
            Ok(())
        }
    }

    fn command_registers(next_interrupts : u32) -> CommandRegisters {
        CommandRegisters { values : [0; 26],
                           interrupts : 0,
                           next_interrupts,
                           command_status : 0,
                           data_status : 0,
                           reads : 0,
                           writes : 0,
                           response_reads : alloc::vec::Vec::new(),
                           started_control : None,
                           fail_read : None,
                           fail_write : None,
                           ignore_argument_zero : false,
                           ignore_control_zero : false }
    }

    fn command_fixture(registers : CommandRegisters) -> Host<CommandRegisters, CommandReady> {
        Host { registers,
               poll_limit : 2,
               _state : PhantomData }
    }

    fn descriptor(index : u8,
                  argument : u32,
                  response : ResponseType)
                  -> CommandDescriptor {
        CommandDescriptor::new(index,
                               argument,
                               response,
                               ResponseValidation::Unchecked,
                               CommandTransfer::None).unwrap()
    }

    fn failed_revalidation(
        result : Result<Host<CommandRegisters, CommandReady>, CommandRecovery<CommandRegisters>>)
        -> CommandRecovery<CommandRegisters> {
        match result {
            Ok(_) => panic!("revalidation unexpectedly returned ready ownership"),
            Err(recovery) => recovery,
        }
    }

    struct ParentRegisters;

    impl clock::RegisterIo for ParentRegisters {
        fn read32(&mut self, offset : usize) -> Result<u32, clock::ClockError> {
            assert_eq!(offset, 0x28);
            Ok(2 << 22)
        }

        fn read64(&mut self, offset : usize) -> Result<u64, clock::ClockError> {
            match offset {
                0x20 => Ok((10u64 << 32) | (2u64 << 26)),
                0x50 => Ok(3 << 20),
                _ => panic!("unexpected clock offset {offset:#x}"),
            }
        }
    }

    fn controller_clock_plan(target_hz : u32) -> ControllerClockPlan {
        let parent = clock::snapshot_consistent(&mut ParentRegisters, 100_000_000).unwrap();
        ControllerClockPlan::from_parent(parent, target_hz).unwrap()
    }

    fn clock_guard() -> ClockTransactionGuard<'static> {
        loop {
            match try_begin_clock_transaction() {
                Ok(guard) => return guard,
                Err(ClockTransactionBusy) => core::hint::spin_loop(),
            }
        }
    }

    #[derive(Default)]
    struct Delay {
        calls : alloc::vec::Vec<u32>,
    }

    impl ResetDelay for Delay {
        fn delay_milliseconds(&mut self, milliseconds : u32) {
            self.calls
                .push(milliseconds);
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum PreflightEvent {
        Read(usize),
        Write(usize, u32),
    }

    struct PreflightRegisters {
        control : u32,
        interrupt_enable : u32,
        command_status : u32,
        data_status : u32,
        events : alloc::vec::Vec<PreflightEvent>,
        reads : usize,
        writes : usize,
        fail_read : Option<usize>,
        fail_write : Option<usize>,
        ignore_control_write : bool,
        ignore_interrupt_enable_write : bool,
    }

    impl RegisterIo for PreflightRegisters {
        fn read32(&mut self, offset : usize) -> Result<u32, MmcError> {
            self.events
                .push(PreflightEvent::Read(offset));
            self.reads += 1;
            if self.fail_read == Some(self.reads) {
                return Err(MmcError::RegisterOutOfRange);
            }
            match offset {
                REG_CTL => Ok(self.control),
                REG_IEN => Ok(self.interrupt_enable),
                REG_CSTS => Ok(self.command_status),
                REG_DSTS => Ok(self.data_status),
                _ => Err(MmcError::RegisterOutOfRange),
            }
        }

        fn write32(&mut self, offset : usize, value : u32) -> Result<(), MmcError> {
            self.events
                .push(PreflightEvent::Write(offset, value));
            self.writes += 1;
            if self.fail_write == Some(self.writes) {
                return Err(MmcError::RegisterOutOfRange);
            }
            match offset {
                REG_CTL => {
                    if !self.ignore_control_write {
                        self.control = value;
                    }
                }
                REG_IEN => {
                    if !self.ignore_interrupt_enable_write {
                        self.interrupt_enable = value;
                    }
                }
                REG_INT => {}
                _ => return Err(MmcError::RegisterOutOfRange),
            }
            Ok(())
        }
    }

    fn preflight_registers() -> PreflightRegisters {
        PreflightRegisters { control : 0,
                             interrupt_enable : 0,
                             command_status : 0,
                             data_status : 0,
                             events : alloc::vec::Vec::new(),
                             reads : 0,
                             writes : 0,
                             fail_read : None,
                             fail_write : None,
                             ignore_control_write : false,
                             ignore_interrupt_enable_write : false }
    }

    fn command_host(registers : MockRegisters) -> Host<MockRegisters, CommandReady> {
        let mut delay = Delay::default();
        let preflight_authority = unsafe { HostPreflightAuthority::assume_board_verified() };
        let host = Host::new(registers, 2).preflight(&mut delay, &preflight_authority)
                                          .unwrap();
        assert_eq!(delay.calls, [10]);
        let clock_authority = unsafe { ControllerClockAuthority::assume_board_verified() };
        let mut guard = clock_guard();
        let (host, _clock) = host.configure_controller_clock(controller_clock_plan(400_000),
                                                             &clock_authority,
                                                             &mut guard)
                                 .unwrap();
        drop(guard);
        host.authorize_host_fixture()
    }

    #[test]
    fn host_preflight_follows_reset_delay_and_idle_sequence() {
        let mut delay = Delay::default();
        let authority = unsafe { HostPreflightAuthority::assume_board_verified() };
        let host = Host::new(preflight_registers(), 2).preflight(&mut delay, &authority)
                                                      .unwrap();
        assert_eq!(delay.calls, [10]);
        let registers = host.into_inner();
        assert_eq!(registers.events,
                   [PreflightEvent::Write(REG_CTL, CTL_RESET),
                    PreflightEvent::Write(REG_CTL, CTL_EXTERNAL_CLOCK),
                    PreflightEvent::Read(REG_CTL),
                    PreflightEvent::Write(REG_INT, INT_CLEAR),
                    PreflightEvent::Write(REG_IEN, INT_CLEAR),
                    PreflightEvent::Read(REG_IEN),
                    PreflightEvent::Read(REG_CSTS),
                    PreflightEvent::Read(REG_DSTS)]);
    }

    #[test]
    fn host_preflight_failures_preserve_session_and_retry_from_reset() {
        let authority = unsafe { HostPreflightAuthority::assume_board_verified() };

        for (write, stage) in [(1, HostPreflightStage::WriteReset),
                               (2, HostPreflightStage::WriteExternalClock),
                               (3, HostPreflightStage::ClearInterrupts),
                               (4, HostPreflightStage::EnableInterrupts)]
        {
            let mut registers = preflight_registers();
            registers.fail_write = Some(write);
            let mut delay = Delay::default();
            let failure = match Host::new(registers, 2).preflight(&mut delay, &authority) {
                Ok(_) => panic!("write failure unexpectedly preflighted"),
                Err(failure) => failure,
            };
            assert_eq!(failure.stage, stage);
            assert_eq!(failure.error,
                       Some(MmcError::RegisterOutOfRange));
        }

        for (read, stage) in [(1, HostPreflightStage::ReadbackControl),
                              (2, HostPreflightStage::ReadbackInterruptEnable),
                              (3, HostPreflightStage::ReadCommandStatus),
                              (4, HostPreflightStage::ReadDataStatus)]
        {
            let mut registers = preflight_registers();
            registers.fail_read = Some(read);
            let mut delay = Delay::default();
            let failure = match Host::new(registers, 2).preflight(&mut delay, &authority) {
                Ok(_) => panic!("read failure unexpectedly preflighted"),
                Err(failure) => failure,
            };
            assert_eq!(failure.stage, stage);
            assert_eq!(failure.error,
                       Some(MmcError::RegisterOutOfRange));
        }

        let mut registers = preflight_registers();
        registers.fail_write = Some(1);
        let mut delay = Delay::default();
        let failure = match Host::new(registers, 2).preflight(&mut delay, &authority) {
            Ok(_) => panic!("reset failure unexpectedly preflighted"),
            Err(failure) => failure,
        };
        assert!(failure.retry(&mut delay, &authority)
                       .is_ok());
        assert_eq!(delay.calls, [10]);
    }

    #[test]
    fn host_preflight_rejects_readback_mismatch_and_bounded_busy_state() {
        let authority = unsafe { HostPreflightAuthority::assume_board_verified() };

        let mut control = preflight_registers();
        control.ignore_control_write = true;
        let failure = match Host::new(control, 2).preflight(&mut Delay::default(), &authority) {
            Ok(_) => panic!("control mismatch unexpectedly preflighted"),
            Err(failure) => failure,
        };
        assert_eq!(failure.stage,
                   HostPreflightStage::ControlMismatch);
        assert_eq!(failure.observed_control, Some(0));

        let mut interrupt_enable = preflight_registers();
        interrupt_enable.ignore_interrupt_enable_write = true;
        let failure =
            match Host::new(interrupt_enable, 2).preflight(&mut Delay::default(), &authority) {
                Ok(_) => panic!("interrupt-enable mismatch unexpectedly preflighted"),
                Err(failure) => failure,
            };
        assert_eq!(failure.stage,
                   HostPreflightStage::InterruptEnableMismatch);

        let mut busy = preflight_registers();
        busy.command_status = CSTS_ON;
        busy.data_status = DSTS_ACTIVE;
        let failure = match Host::new(busy, 2).preflight(&mut Delay::default(), &authority) {
            Ok(_) => panic!("busy controller unexpectedly preflighted"),
            Err(failure) => failure,
        };
        assert_eq!(failure.stage,
                   HostPreflightStage::IdleTimeout);
        assert_eq!(failure.observed_command_status,
                   Some(CSTS_ON));
        assert_eq!(failure.observed_data_status,
                   Some(DSTS_ACTIVE));
    }

    impl RegisterIo for MockRegisters {
        fn read32(&mut self, offset : usize) -> Result<u32, MmcError> {
            if offset == REG_INT {
                return Ok(self.interrupts);
            }
            self.values
                .get(offset / 4)
                .copied()
                .ok_or(MmcError::RegisterOutOfRange)
        }
        fn write32(&mut self, offset : usize, value : u32) -> Result<(), MmcError> {
            if offset == REG_INT {
                return Ok(());
            }
            *self.values
                 .get_mut(offset / 4)
                 .ok_or(MmcError::RegisterOutOfRange)? = value;
            Ok(())
        }
    }

    #[test]
    fn programs_bounded_clock_and_non_data_command() {
        let mut registers = MockRegisters { values : [0; 26],
                                            interrupts : INT_COMMAND_SENT };
        registers.values[REG_RSP0 / 4] = 0x1234;
        let outcome = command_host(registers).execute_command(descriptor(8,
                                                                         0x1AA,
                                                                         ResponseType::Short));
        let (host, response) = match outcome {
            CommandOutcome::Completed { host, response } => (host, response),
            _ => panic!("command did not complete"),
        };
        assert_eq!(response, CommandResponse::Short(0x1234));
        let registers = host.into_inner();
        assert_eq!(registers.values[REG_PRE / 4],
                   PRE_ENABLE | 255);
        assert_eq!(registers.values[REG_CARG / 4], 0);
        assert_eq!(registers.values[REG_CCTL / 4], 0);
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum ClockEvent {
        Read(usize),
        Write(usize, u32),
    }

    struct ClockRegisters {
        pre : u32,
        control : u32,
        events : alloc::vec::Vec<ClockEvent>,
        reads : usize,
        writes : usize,
        fail_read : Option<usize>,
        fail_write : Option<usize>,
        ignore_pre_write : bool,
        ignore_control_write : bool,
    }

    impl RegisterIo for ClockRegisters {
        fn read32(&mut self, offset : usize) -> Result<u32, MmcError> {
            self.events
                .push(ClockEvent::Read(offset));
            self.reads += 1;
            if self.fail_read == Some(self.reads) {
                return Err(MmcError::RegisterOutOfRange);
            }
            match offset {
                REG_PRE => Ok(self.pre),
                REG_CTL => Ok(self.control),
                _ => Err(MmcError::RegisterOutOfRange),
            }
        }

        fn write32(&mut self, offset : usize, value : u32) -> Result<(), MmcError> {
            self.events
                .push(ClockEvent::Write(offset, value));
            self.writes += 1;
            if self.fail_write == Some(self.writes) {
                return Err(MmcError::RegisterOutOfRange);
            }
            match offset {
                REG_PRE => {
                    if !self.ignore_pre_write {
                        self.pre = value;
                    }
                }
                REG_CTL => {
                    if !self.ignore_control_write {
                        self.control = value;
                    }
                }
                _ => return Err(MmcError::RegisterOutOfRange),
            }
            Ok(())
        }
    }

    fn clock_registers(pre : u32, control : u32) -> ClockRegisters {
        ClockRegisters { pre,
                         control,
                         events : alloc::vec::Vec::new(),
                         reads : 0,
                         writes : 0,
                         fail_read : None,
                         fail_write : None,
                         ignore_pre_write : false,
                         ignore_control_write : false }
    }

    #[test]
    fn controller_clock_transaction_is_minimal_and_serialized() {
        let plan = controller_clock_plan(400_000);
        assert_eq!(plan.parent_hz(), 125_000_000);
        assert_eq!(plan.requested_hz(), 400_000);
        assert_eq!(plan.divider(), 255);

        let authority = unsafe { ControllerClockAuthority::assume_board_verified() };
        let mut guard = clock_guard();
        assert!(matches!(try_begin_clock_transaction(),
                         Err(ClockTransactionBusy)));
        let mut registers = clock_registers(0x55AA, 1 << 9);
        let ready = apply_controller_clock(&mut registers,
                                           plan,
                                           &authority,
                                           &mut guard).unwrap();
        assert_eq!(ready.plan(), plan);
        assert_eq!(registers.pre, PRE_ENABLE | 255);
        assert_eq!(registers.control,
                   (1 << 9) | CTL_ENABLE_CLOCK);
        assert_eq!(registers.events,
                   [ClockEvent::Read(REG_PRE),
                    ClockEvent::Read(REG_CTL),
                    ClockEvent::Write(REG_PRE, PRE_ENABLE | 255),
                    ClockEvent::Read(REG_PRE),
                    ClockEvent::Write(REG_CTL, (1 << 9) | CTL_ENABLE_CLOCK),
                    ClockEvent::Read(REG_CTL)]);
        drop(guard);

        let mut guard = clock_guard();
        registers.events
                 .clear();
        apply_controller_clock(&mut registers,
                               plan,
                               &authority,
                               &mut guard).unwrap();
        assert_eq!(registers.events,
                   [ClockEvent::Read(REG_PRE),
                    ClockEvent::Read(REG_CTL)]);
    }

    #[test]
    fn controller_clock_failures_remain_revalidatable() {
        let plan = controller_clock_plan(25_000_000);
        let authority = unsafe { ControllerClockAuthority::assume_board_verified() };
        let mut guard = clock_guard();

        for (read, stage) in [(1, ControllerClockStage::PreflightPre),
                              (2, ControllerClockStage::PreflightControl),
                              (3, ControllerClockStage::ReadbackPre),
                              (4, ControllerClockStage::ReadbackControl)]
        {
            let mut registers = clock_registers(0, 0);
            registers.fail_read = Some(read);
            let recovery = apply_controller_clock(&mut registers,
                                                  plan,
                                                  &authority,
                                                  &mut guard).unwrap_err();
            assert_eq!(recovery.stage, stage);
            assert_eq!(recovery.error,
                       Some(MmcError::RegisterOutOfRange));
        }

        let mut failed_pre = clock_registers(0, 0);
        failed_pre.fail_write = Some(1);
        let recovery = apply_controller_clock(&mut failed_pre,
                                              plan,
                                              &authority,
                                              &mut guard).unwrap_err();
        assert_eq!(recovery.stage,
                   ControllerClockStage::WritePre);
        assert_eq!(recovery.observed_pre, None);

        let mut mismatch = clock_registers(0, 0);
        mismatch.ignore_pre_write = true;
        let recovery = apply_controller_clock(&mut mismatch,
                                              plan,
                                              &authority,
                                              &mut guard).unwrap_err();
        assert_eq!(recovery.stage,
                   ControllerClockStage::ReadbackPreMismatch);
        assert_eq!(recovery.observed_pre, Some(0));

        mismatch.pre = plan.pre_value();
        mismatch.control = CTL_ENABLE_CLOCK;
        assert_eq!(recovery.revalidate(&mut mismatch, &mut guard)
                           .unwrap()
                           .plan(),
                   plan);

        let mut failed_control = clock_registers(plan.pre_value(), 0x80);
        failed_control.fail_write = Some(1);
        let recovery = apply_controller_clock(&mut failed_control,
                                              plan,
                                              &authority,
                                              &mut guard).unwrap_err();
        assert_eq!(recovery.stage,
                   ControllerClockStage::WriteControl);
        assert_eq!(recovery.observed_pre,
                   Some(plan.pre_value()));

        let mut control_mismatch = clock_registers(plan.pre_value(), 0x80);
        control_mismatch.ignore_control_write = true;
        let recovery = apply_controller_clock(&mut control_mismatch,
                                              plan,
                                              &authority,
                                              &mut guard).unwrap_err();
        assert_eq!(recovery.stage,
                   ControllerClockStage::ReadbackControlMismatch);
        assert_eq!(recovery.observed_control, Some(0x80));
    }

    #[test]
    fn bounds_polling_and_reports_command_errors() {
        let outcome = command_host(MockRegisters { values : [0; 26],
                                                   interrupts : 0 }).execute_command(descriptor(0,
                                                                                                0,
                                                                                                ResponseType::None));
        match outcome {
            CommandOutcome::RecoveryRequired(recovery) => {
                assert_eq!(recovery.stage, CommandStage::PollTimeout);
                assert_eq!(recovery.error, MmcError::Timeout);
            }
            _ => panic!("poll timeout did not require recovery"),
        }
        let outcome = command_host(MockRegisters { values : [0; 26],
                                                   interrupts:
                                                       INT_COMMAND_TIMEOUT }).execute_command(descriptor(8,
                                                                                                         0,
                                                                                                         ResponseType::Short));
        match outcome {
            CommandOutcome::RecoveryRequired(recovery) => {
                assert_eq!(recovery.stage,
                           CommandStage::CommandTimeout);
                assert_eq!(recovery.error,
                           MmcError::ResponseTimeout);
            }
            _ => panic!("command timeout did not require recovery"),
        }
        let outcome = command_host(MockRegisters { values : [0; 26],
                                                   interrupts:
                                                       INT_RESPONSE_CRC }).execute_command(descriptor(8,
                                                                                                      0,
                                                                                                      ResponseType::Short));
        match outcome {
            CommandOutcome::RecoveryRequired(recovery) => {
                assert_eq!(recovery.stage, CommandStage::ResponseCrc);
                assert_eq!(recovery.error, MmcError::ResponseCrc);
            }
            _ => panic!("response CRC did not require recovery"),
        }
    }

    #[test]
    fn command_descriptor_rejects_unsupported_intent_before_host_mmio() {
        assert_eq!(CommandDescriptor::new(64,
                                          0,
                                          ResponseType::None,
                                          ResponseValidation::Unchecked,
                                          CommandTransfer::None),
                   Err(CommandDescriptorError::InvalidIndex));
        assert_eq!(CommandDescriptor::new(8,
                                          0,
                                          ResponseType::Short,
                                          ResponseValidation::Unchecked,
                                          CommandTransfer::Data),
                   Err(CommandDescriptorError::DataUnsupported));
        for response in [ResponseType::Short, ResponseType::Long] {
            for validation in [ResponseValidation::Crc,
                               ResponseValidation::CrcAndBusy]
            {
                assert_eq!(CommandDescriptor::new(8,
                                                  0,
                                                  response,
                                                  validation,
                                                  CommandTransfer::None),
                           Err(CommandDescriptorError::ResponsePolicyUnsupported));
            }
        }

        let mut registers = command_registers(INT_COMMAND_SENT);
        registers.values[REG_RSP0 / 4] = 1;
        registers.values[REG_RSP1 / 4] = 2;
        registers.values[REG_RSP2 / 4] = 3;
        registers.values[REG_RSP3 / 4] = 4;
        match command_fixture(registers).execute_command(descriptor(2, 0, ResponseType::Long)) {
            CommandOutcome::Completed { host, response } => {
                assert_eq!(response,
                           CommandResponse::Long([1, 2, 3, 4]));
                let registers = host.into_inner();
                assert_eq!(registers.interrupts, 0);
                assert_eq!(registers.response_reads,
                           [REG_RSP0, REG_RSP1, REG_RSP2, REG_RSP3]);
                assert_eq!(registers.started_control,
                           Some(2 | CCTL_HOST | CCTL_START | CCTL_WAIT_RESPONSE |
                                CCTL_LONG_RESPONSE));
                assert_eq!(registers.started_control.unwrap() & (1 << 13),
                           0);
                assert_eq!(registers.values[REG_CARG / 4], 0);
                assert_eq!(registers.values[REG_CCTL / 4], 0);
            }
            _ => panic!("valid command did not return ready ownership"),
        }

        for (failed_write, stage) in [(1, CommandStage::ClearInterrupts),
                                      (2, CommandStage::WriteArgument),
                                      (3, CommandStage::StartCommand),
                                      (4, CommandStage::AcknowledgeCompletion),
                                      (5, CommandStage::CleanupArgument),
                                      (6, CommandStage::CleanupControl)]
        {
            let mut registers = command_registers(INT_COMMAND_SENT);
            registers.fail_write = Some(failed_write);
            match command_fixture(registers).execute_command(descriptor(8,
                                                                         0,
                                                                         ResponseType::Short)) {
                CommandOutcome::RecoveryRequired(recovery) => {
                    assert_eq!(recovery.stage, stage);
                    assert_eq!(recovery.error,
                               MmcError::RegisterOutOfRange);
                }
                _ => panic!("write fault did not isolate command ownership"),
            }
        }

        for (failed_read, stage) in [(1, CommandStage::PollInterrupts),
                                     (2, CommandStage::ReadResponse0),
                                     (3, CommandStage::ReadResponse1),
                                     (4, CommandStage::ReadResponse2),
                                     (5, CommandStage::ReadResponse3)]
        {
            let mut registers = command_registers(INT_COMMAND_SENT);
            registers.fail_read = Some(failed_read);
            match command_fixture(registers).execute_command(descriptor(2,
                                                                         0,
                                                                         ResponseType::Long)) {
                CommandOutcome::RecoveryRequired(recovery) => {
                    assert_eq!(recovery.stage, stage);
                    assert_eq!(recovery.error,
                               MmcError::RegisterOutOfRange);
                }
                _ => panic!("read fault did not isolate command ownership"),
            }
        }
    }

    #[test]
    fn command_response_contract_minimizes_register_reads() {
        let outcome = command_fixture(command_registers(INT_COMMAND_SENT)).execute_command(
            descriptor(0, 0, ResponseType::None));
        match outcome {
            CommandOutcome::Completed { host, response } => {
                assert_eq!(response, CommandResponse::None);
                let registers = host.into_inner();
                assert!(registers.response_reads.is_empty());
                assert_eq!(registers.started_control,
                           Some(CCTL_HOST | CCTL_START));
                assert_eq!(registers.values[REG_CCTL / 4], 0);
            }
            _ => panic!("no-response command did not complete"),
        }

        let mut registers = command_registers(INT_COMMAND_SENT);
        registers.values[REG_RSP0 / 4] = 0xCAFE;
        let outcome = command_fixture(registers).execute_command(descriptor(8,
                                                                            0,
                                                                            ResponseType::Short));
        match outcome {
            CommandOutcome::Completed { host, response } => {
                assert_eq!(response,
                           CommandResponse::Short(0xCAFE));
                let registers = host.into_inner();
                assert_eq!(registers.response_reads, [REG_RSP0]);
                assert_eq!(registers.started_control,
                           Some(8 | CCTL_HOST | CCTL_START | CCTL_WAIT_RESPONSE));
                assert_eq!(registers.values[REG_CCTL / 4], 0);
            }
            _ => panic!("short-response command did not complete"),
        }
    }

    #[test]
    fn command_error_bits_precede_completion_and_busyend_is_not_completion() {
        let outcome = command_fixture(command_registers(INT_COMMAND_TIMEOUT |
                                                         INT_RESPONSE_CRC |
                                                         INT_COMMAND_SENT)).execute_command(
            descriptor(8, 0, ResponseType::Short));
        match outcome {
            CommandOutcome::RecoveryRequired(recovery) => {
                assert_eq!(recovery.stage,
                           CommandStage::CommandTimeout);
            }
            _ => panic!("timeout did not take priority over completion"),
        }

        let outcome = command_fixture(command_registers(INT_RESPONSE_CRC |
                                                         INT_COMMAND_SENT)).execute_command(
            descriptor(8, 0, ResponseType::Short));
        match outcome {
            CommandOutcome::RecoveryRequired(recovery) => {
                assert_eq!(recovery.stage, CommandStage::ResponseCrc);
            }
            _ => panic!("CRC status did not take priority over completion"),
        }

        let outcome = command_fixture(command_registers(1 << 9)).execute_command(
            descriptor(8, 0, ResponseType::Short));
        match outcome {
            CommandOutcome::RecoveryRequired(recovery) => {
                assert_eq!(recovery.stage, CommandStage::PollTimeout);
            }
            _ => panic!("BUSYEND alone was incorrectly accepted as command completion"),
        }
    }

    #[test]
    fn command_recovery_requires_idle_and_verified_interrupt_clear() {
        let recovery =
            match command_fixture(command_registers(INT_COMMAND_TIMEOUT)).execute_command(descriptor(8,
                                                                                                      0,
                                                                                                      ResponseType::Short)) {
                CommandOutcome::RecoveryRequired(recovery) => recovery,
                _ => panic!("timeout did not require recovery"),
            };
        let host = recovery.revalidate()
                           .expect("idle W1C state should revalidate");
        assert_eq!(host.into_inner().interrupts, 0);

        let mut busy = command_registers(0);
        busy.command_status = CSTS_ON;
        busy.data_status = DSTS_ACTIVE;
        let recovery = match command_fixture(busy).execute_command(descriptor(0,
                                                                               0,
                                                                               ResponseType::None)) {
            CommandOutcome::RecoveryRequired(recovery) => recovery,
            _ => panic!("poll timeout did not require recovery"),
        };
        let recovery = failed_revalidation(recovery.revalidate());
        assert_eq!(recovery.origin_stage,
                   CommandStage::PollTimeout);
        assert_eq!(recovery.origin_error, MmcError::Timeout);
        assert_eq!(recovery.stage, CommandStage::RevalidateBusy);
        assert_eq!(recovery.observed_command_status,
                   Some(CSTS_ON));
        assert_eq!(recovery.observed_data_status,
                   Some(DSTS_ACTIVE));
        let _must_preflight_again : Host<_, Uninitialized> = recovery.into_uninitialized();

        let unknown = 1 << 12;
        let recovery =
            match command_fixture(command_registers(unknown)).execute_command(descriptor(0,
                                                                                          0,
                                                                                          ResponseType::None)) {
                CommandOutcome::RecoveryRequired(recovery) => recovery,
                _ => panic!("unknown status did not reach recovery"),
            };
        let recovery = failed_revalidation(recovery.revalidate());
        assert_eq!(recovery.stage,
                   CommandStage::RevalidateUnknownInterrupt);
        assert_eq!(recovery.observed_interrupts, Some(unknown));
    }

    #[test]
    fn command_revalidation_fault_matrix_never_returns_ready() {
        for (failed_read, stage) in [(2, CommandStage::RevalidateCommandStatus),
                                     (3, CommandStage::RevalidateDataStatus),
                                     (4, CommandStage::RevalidateInterrupts),
                                     (5, CommandStage::RevalidateInterruptReadback),
                                     (6, CommandStage::RevalidateArgumentReadback),
                                     (7, CommandStage::RevalidateControlReadback)]
        {
            let mut registers = command_registers(INT_COMMAND_TIMEOUT);
            registers.fail_read = Some(failed_read);
            let recovery =
                match command_fixture(registers).execute_command(descriptor(8,
                                                                             0,
                                                                             ResponseType::Short)) {
                    CommandOutcome::RecoveryRequired(recovery) => recovery,
                    _ => panic!("timeout did not require recovery"),
                };
            let recovery = failed_revalidation(recovery.revalidate());
            assert_eq!(recovery.origin_stage,
                       CommandStage::CommandTimeout);
            assert_eq!(recovery.origin_error,
                       MmcError::ResponseTimeout);
            assert_eq!(recovery.stage, stage);
            assert_eq!(recovery.error,
                       MmcError::RegisterOutOfRange);
        }

        for (failed_write, stage) in [(4, CommandStage::RevalidateClearInterrupts),
                                      (5, CommandStage::RevalidateCleanupArgument),
                                      (6, CommandStage::RevalidateCleanupControl)]
        {
            let mut registers = command_registers(INT_COMMAND_TIMEOUT);
            registers.fail_write = Some(failed_write);
            let recovery =
                match command_fixture(registers).execute_command(descriptor(8,
                                                                             0,
                                                                             ResponseType::Short)) {
                    CommandOutcome::RecoveryRequired(recovery) => recovery,
                    _ => panic!("timeout did not require recovery"),
                };
            let recovery = failed_revalidation(recovery.revalidate());
            assert_eq!(recovery.stage, stage);
            assert_eq!(recovery.error,
                       MmcError::RegisterOutOfRange);
        }

        for (argument, control, stage) in [(true,
                                            false,
                                            CommandStage::RevalidateArgumentMismatch),
                                           (false,
                                            true,
                                            CommandStage::RevalidateControlMismatch)]
        {
            let mut registers = command_registers(INT_COMMAND_TIMEOUT);
            registers.ignore_argument_zero = argument;
            registers.ignore_control_zero = control;
            let recovery =
                match command_fixture(registers).execute_command(descriptor(8,
                                                                             0x55,
                                                                             ResponseType::Short)) {
                    CommandOutcome::RecoveryRequired(recovery) => recovery,
                    _ => panic!("timeout did not require recovery"),
                };
            let recovery = failed_revalidation(recovery.revalidate());
            assert_eq!(recovery.stage, stage);
        }
    }

    #[test]
    fn mmc_irq_ack_reads_then_clears_known_w1c_bits_before_rearm() {
        let irq = GlobalIrq::from_bank_local(0, 31).unwrap();
        let mut registers = ack_registers(INT_COMMAND_SENT | INT_RESPONSE_CRC);
        let disposition = acknowledge_interrupt(&mut registers, irq, acknowledged(irq)).unwrap();
        assert_eq!(disposition,
                   IrqDisposition::Rearm(DeviceAckedIrq::after_device_clear(irq)));
        assert_eq!(registers.events,
                   [AckEvent::Read(REG_INT),
                    AckEvent::Write(REG_INT,
                                    INT_COMMAND_SENT | INT_RESPONSE_CRC)]);
    }

    #[test]
    fn mmc_irq_ack_keeps_unknown_or_failed_sources_masked_and_recoverable() {
        let irq = GlobalIrq::from_bank_local(0, 31).unwrap();
        let other = GlobalIrq::from_bank_local(0, 30).unwrap();
        let mut registers = ack_registers(INT_COMMAND_SENT);
        let failure = acknowledge_interrupt(&mut registers, irq, acknowledged(other)).unwrap_err();
        assert_eq!(failure.error,
                   MmcIrqAckError::UnexpectedSource);
        assert!(registers.events
                         .is_empty());
        assert_eq!(failure.acknowledged
                          .irq(),
                   other);

        let mut registers = ack_registers(0);
        let failure = acknowledge_interrupt(&mut registers, irq, acknowledged(irq)).unwrap_err();
        assert_eq!(failure.error,
                   MmcIrqAckError::NoKnownPending);
        assert_eq!(registers.events,
                   [AckEvent::Read(REG_INT)]);

        let mut registers = ack_registers(INT_COMMAND_SENT);
        registers.fail_read = true;
        let failure = acknowledge_interrupt(&mut registers, irq, acknowledged(irq)).unwrap_err();
        assert_eq!(failure.error,
                   MmcIrqAckError::Io(MmcError::RegisterOutOfRange));
        assert_eq!(registers.events,
                   [AckEvent::Read(REG_INT)]);

        let unknown = 1 << 15;
        let mut registers = ack_registers(INT_COMMAND_SENT | unknown);
        let failure = acknowledge_interrupt(&mut registers, irq, acknowledged(irq)).unwrap_err();
        assert_eq!(failure.error,
                   MmcIrqAckError::UnknownPending(unknown));
        assert_eq!(registers.events,
                   [AckEvent::Read(REG_INT),
                    AckEvent::Write(REG_INT, INT_COMMAND_SENT)]);
        assert_eq!(failure.acknowledged
                          .irq(),
                   irq);

        let mut registers = ack_registers(INT_COMMAND_TIMEOUT);
        registers.fail_write = true;
        let failure = acknowledge_interrupt(&mut registers, irq, acknowledged(irq)).unwrap_err();
        assert_eq!(failure.error,
                   MmcIrqAckError::Io(MmcError::RegisterOutOfRange));
        assert_eq!(failure.acknowledged
                          .irq(),
                   irq);
    }
}

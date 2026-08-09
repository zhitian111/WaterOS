//! Deferred 2K1000LA MMC bring-up planning.
//!
//! Linux's dedicated `loongson2-mmc` driver proves this is not a DesignWare
//! register layout. The second DT register is an APB-DMA routing register, not
//! a FIFO window. WaterOS reuses [`dw_mmc::sd`] only as an SD protocol layer.

use crate::{irq_domain::{AcknowledgedIrq, DeviceAckedIrq, GlobalIrq, IrqDisposition},
            topology::MmcDescription};
use api_v0::MmioRegion;
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
    InterruptPathUnverified,
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
    pub blockers : [ActivationBlocker; 6],
}

impl BringUpPlan {
    pub const fn can_activate(&self) -> bool { false }
}

pub fn plan(description : &MmcDescription) -> Result<BringUpPlan, PlanError> {
    if description.controller_mmio.size < MIN_CONTROLLER_WINDOW {
        return Err(PlanError::ControllerWindowTooSmall);
    }
    let auxiliary_mmio = description.auxiliary_mmio
                                    .filter(|region| region.size >= 4)
                                    .ok_or(PlanError::MissingAuxiliaryWindow)?;
    if description.clocks.len() != 1 {
        return Err(PlanError::MissingClock);
    }
    Ok(BringUpPlan {
        controller_mmio : description.controller_mmio,
        auxiliary_mmio,
        bus_width : description.bus_width,
        blockers : [ActivationBlocker::DataPathUnavailable,
                    ActivationBlocker::ExternalDmaExecutorUnavailable,
                    ActivationBlocker::ClockControlUnavailable,
                    ActivationBlocker::PowerSequencingUnavailable,
                    ActivationBlocker::CardDetectUnavailable,
                    ActivationBlocker::InterruptPathUnverified],
    })
}

const REG_CTL : usize = 0x00;
const REG_PRE : usize = 0x04;
const REG_CARG : usize = 0x08;
const REG_CCTL : usize = 0x0c;
const REG_RSP0 : usize = 0x14;
const REG_RSP1 : usize = 0x18;
const REG_RSP2 : usize = 0x1c;
const REG_RSP3 : usize = 0x20;
const REG_INT : usize = 0x3c;

const CTL_ENABLE_CLOCK : u32 = 1 << 0;
const PRE_ENABLE : u32 = 1 << 31;
const CCTL_HOST : u32 = 1 << 6;
const CCTL_START : u32 = 1 << 8;
const CCTL_WAIT_RESPONSE : u32 = 1 << 9;
const CCTL_LONG_RESPONSE : u32 = 1 << 10;
const INT_COMMAND_SENT : u32 = 1 << 6;
const INT_COMMAND_TIMEOUT : u32 = 1 << 7;
const INT_RESPONSE_CRC : u32 = 1 << 8;
const INT_CLEAR : u32 = 0x3ff;

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

/// Clear a masked/acknowledged MMC interrupt and produce rearm evidence only
/// when every observed pending bit has documented W1C behavior.
pub fn acknowledge_interrupt<R : RegisterIo>(registers : &mut R,
                                              expected : GlobalIrq,
                                              acknowledged : AcknowledgedIrq)
                                              -> Result<IrqDisposition,
                                                        MmcIrqAckFailure> {
    if acknowledged.irq() != expected {
        return Err(MmcIrqAckFailure { error : MmcIrqAckError::UnexpectedSource,
                                     acknowledged });
    }
    let status = match registers.read32(REG_INT) {
        Ok(status) => status,
        Err(error) => return Err(MmcIrqAckFailure { error : MmcIrqAckError::Io(error),
                                                   acknowledged }),
    };
    let known = status & INT_CLEAR;
    if known == 0 {
        return Err(MmcIrqAckFailure { error : MmcIrqAckError::NoKnownPending,
                                     acknowledged });
    }
    if let Err(error) = registers.write32(REG_INT, known) {
        return Err(MmcIrqAckFailure { error : MmcIrqAckError::Io(error), acknowledged });
    }
    let unknown = status & !INT_CLEAR;
    if unknown != 0 {
        return Err(MmcIrqAckFailure { error : MmcIrqAckError::UnknownPending(unknown),
                                     acknowledged });
    }
    Ok(IrqDisposition::Rearm(DeviceAckedIrq::after_device_clear(expected)))
}

pub struct Host<R> {
    registers : R,
    poll_limit : usize,
}

/// Match the upstream driver's integer prescaler policy: round upward and clamp
/// at 255. If the requested divisor exceeds 255, the returned actual clock can
/// exceed the target; callers must inspect it before touching hardware.
pub fn clock_prescaler(input_hz : u32, target_hz : u32) -> Result<(u8, u32), MmcError> {
    if input_hz == 0 || target_hz == 0 {
        return Err(MmcError::InvalidParameter);
    }
    let divider = input_hz.div_ceil(target_hz).clamp(1, 255);
    Ok((divider as u8, input_hz / divider))
}

impl<R : RegisterIo> Host<R> {
    pub fn new(registers : R, poll_limit : usize) -> Self {
        Self { registers, poll_limit : poll_limit.max(1) }
    }

    pub fn configure_clock(&mut self,
                           input_hz : u32,
                           target_hz : u32)
                           -> Result<u32, MmcError> {
        let (divider, actual) = clock_prescaler(input_hz, target_hz)?;
        self.registers.write32(REG_PRE, PRE_ENABLE | divider as u32)?;
        let control = self.registers.read32(REG_CTL)?;
        self.registers.write32(REG_CTL, control | CTL_ENABLE_CLOCK)?;
        Ok(actual)
    }

    /// Execute a non-data command by bounded polling.
    ///
    /// Register definitions and W1C interrupt behavior follow the upstream
    /// Linux driver. Physical response ordering remains UNVERIFIED_ON_HARDWARE.
    pub fn execute_command(&mut self,
                           index : u8,
                           argument : u32,
                           response_expected : bool,
                           response_long : bool)
                           -> Result<[u32; 4], MmcError> {
        if index > 63 || response_long && !response_expected {
            return Err(MmcError::InvalidParameter);
        }
        self.registers.write32(REG_INT, INT_CLEAR)?;
        self.registers.write32(REG_CARG, argument)?;
        let mut control = index as u32 | CCTL_HOST | CCTL_START;
        if response_expected { control |= CCTL_WAIT_RESPONSE; }
        if response_long { control |= CCTL_LONG_RESPONSE; }
        self.registers.write32(REG_CCTL, control)?;
        for _ in 0..self.poll_limit {
            let interrupts = self.registers.read32(REG_INT)?;
            if interrupts & INT_COMMAND_TIMEOUT != 0 {
                return Err(MmcError::ResponseTimeout);
            }
            if interrupts & INT_RESPONSE_CRC != 0 {
                return Err(MmcError::ResponseCrc);
            }
            if interrupts & INT_COMMAND_SENT != 0 {
                self.registers.write32(REG_INT, interrupts & INT_CLEAR)?;
                return Ok([self.registers.read32(REG_RSP0)?,
                           self.registers.read32(REG_RSP1)?,
                           self.registers.read32(REG_RSP2)?,
                           self.registers.read32(REG_RSP3)?]);
            }
        }
        Err(MmcError::Timeout)
    }

    #[cfg(test)]
    fn into_inner(self) -> R { self.registers }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topology::{CardDetect, InterruptSpec, NamedResource, ResourceSpecifier};
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
            self.events.push(AckEvent::Read(offset));
            if self.fail_read { Err(MmcError::RegisterOutOfRange) } else { Ok(self.status) }
        }
        fn write32(&mut self, offset : usize, value : u32) -> Result<(), MmcError> {
            self.events.push(AckEvent::Write(offset, value));
            if self.fail_write { Err(MmcError::RegisterOutOfRange) } else { Ok(()) }
        }
    }

    fn ack_registers(status : u32) -> AckRegisters {
        AckRegisters { status, fail_read : false, fail_write : false, events : alloc::vec::Vec::new() }
    }

    fn acknowledged(irq : GlobalIrq) -> AcknowledgedIrq {
        AcknowledgedIrq::after_mask_ack(irq)
    }

    fn description() -> MmcDescription {
        MmcDescription {
            controller_mmio : MmioRegion { base : 0x1fe2_c000, size : 0x68 },
            auxiliary_mmio : Some(MmioRegion { base : 0x1fe0_0438, size : 8 }),
            interrupt : InterruptSpec { parent_phandle : 1,
                                        cells : [31, 4, 0, 0],
                                        cell_count : 2 },
            clocks : vec![NamedResource {
                name : None,
                specifier : ResourceSpecifier { provider_phandle : 2, args : vec![0] },
            }],
            dma : None,
            bus_width : 4,
            card_detect : CardDetect::NonRemovable,
            vmmc_supply : None,
            vqmmc_supply : None,
        }
    }

    #[test]
    fn preserves_dma_routing_window_but_refuses_activation() {
        let plan = plan(&description()).unwrap();
        assert_eq!(plan.controller_mmio.size, 0x68);
        assert_eq!(plan.auxiliary_mmio.base, 0x1fe0_0438);
        assert_eq!(plan.bus_width, 4);
        assert!(!plan.can_activate());
        assert!(plan.blockers.contains(&ActivationBlocker::DataPathUnavailable));
    }

    #[test]
    fn rejects_resources_that_cannot_back_future_register_io() {
        let mut value = description();
        value.auxiliary_mmio = None;
        assert_eq!(plan(&value), Err(PlanError::MissingAuxiliaryWindow));

        let mut value = description();
        value.controller_mmio.size = 0x64;
        assert_eq!(plan(&value), Err(PlanError::ControllerWindowTooSmall));

        let mut value = description();
        value.clocks.clear();
        assert_eq!(plan(&value), Err(PlanError::MissingClock));
    }

    struct MockRegisters {
        values : [u32; 26],
        interrupts : u32,
    }
    impl RegisterIo for MockRegisters {
        fn read32(&mut self, offset : usize) -> Result<u32, MmcError> {
            if offset == REG_INT { return Ok(self.interrupts); }
            self.values.get(offset / 4).copied().ok_or(MmcError::RegisterOutOfRange)
        }
        fn write32(&mut self, offset : usize, value : u32) -> Result<(), MmcError> {
            if offset == REG_INT { return Ok(()); }
            *self.values.get_mut(offset / 4).ok_or(MmcError::RegisterOutOfRange)? = value;
            Ok(())
        }
    }

    #[test]
    fn programs_bounded_clock_and_non_data_command() {
        let mut registers = MockRegisters { values : [0; 26],
                                            interrupts : INT_COMMAND_SENT };
        registers.values[REG_RSP0 / 4] = 0x1234;
        let mut host = Host::new(registers, 2);
        assert_eq!(host.configure_clock(125_000_000, 400_000), Ok(490_196));
        assert_eq!(host.execute_command(8, 0x1aa, true, false),
                   Ok([0x1234, 0, 0, 0]));
        let registers = host.into_inner();
        assert_eq!(registers.values[REG_PRE / 4], PRE_ENABLE | 255);
        assert_eq!(registers.values[REG_CARG / 4], 0x1aa);
        assert_eq!(registers.values[REG_CCTL / 4],
                   8 | CCTL_HOST | CCTL_START | CCTL_WAIT_RESPONSE);
    }

    #[test]
    fn bounds_polling_and_reports_command_errors() {
        let mut host = Host::new(MockRegisters { values : [0; 26], interrupts : 0 }, 2);
        assert_eq!(host.execute_command(0, 0, false, false), Err(MmcError::Timeout));
        let mut host = Host::new(MockRegisters { values : [0; 26],
                                                interrupts : INT_COMMAND_TIMEOUT }, 2);
        assert_eq!(host.execute_command(8, 0, true, false), Err(MmcError::ResponseTimeout));
        let mut host = Host::new(MockRegisters { values : [0; 26],
                                                interrupts : INT_RESPONSE_CRC }, 2);
        assert_eq!(host.execute_command(8, 0, true, false), Err(MmcError::ResponseCrc));
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
                    AckEvent::Write(REG_INT, INT_COMMAND_SENT | INT_RESPONSE_CRC)]);
    }

    #[test]
    fn mmc_irq_ack_keeps_unknown_or_failed_sources_masked_and_recoverable() {
        let irq = GlobalIrq::from_bank_local(0, 31).unwrap();
        let other = GlobalIrq::from_bank_local(0, 30).unwrap();
        let mut registers = ack_registers(INT_COMMAND_SENT);
        let failure = acknowledge_interrupt(&mut registers, irq, acknowledged(other)).unwrap_err();
        assert_eq!(failure.error, MmcIrqAckError::UnexpectedSource);
        assert!(registers.events.is_empty());
        assert_eq!(failure.acknowledged.irq(), other);

        let mut registers = ack_registers(0);
        let failure = acknowledge_interrupt(&mut registers, irq, acknowledged(irq)).unwrap_err();
        assert_eq!(failure.error, MmcIrqAckError::NoKnownPending);
        assert_eq!(registers.events, [AckEvent::Read(REG_INT)]);

        let mut registers = ack_registers(INT_COMMAND_SENT);
        registers.fail_read = true;
        let failure = acknowledge_interrupt(&mut registers, irq, acknowledged(irq)).unwrap_err();
        assert_eq!(failure.error, MmcIrqAckError::Io(MmcError::RegisterOutOfRange));
        assert_eq!(registers.events, [AckEvent::Read(REG_INT)]);

        let unknown = 1 << 15;
        let mut registers = ack_registers(INT_COMMAND_SENT | unknown);
        let failure = acknowledge_interrupt(&mut registers, irq, acknowledged(irq)).unwrap_err();
        assert_eq!(failure.error, MmcIrqAckError::UnknownPending(unknown));
        assert_eq!(registers.events,
                   [AckEvent::Read(REG_INT), AckEvent::Write(REG_INT, INT_COMMAND_SENT)]);
        assert_eq!(failure.acknowledged.irq(), irq);

        let mut registers = ack_registers(INT_COMMAND_TIMEOUT);
        registers.fail_write = true;
        let failure = acknowledge_interrupt(&mut registers, irq, acknowledged(irq)).unwrap_err();
        assert_eq!(failure.error, MmcIrqAckError::Io(MmcError::RegisterOutOfRange));
        assert_eq!(failure.acknowledged.irq(), irq);
    }
}

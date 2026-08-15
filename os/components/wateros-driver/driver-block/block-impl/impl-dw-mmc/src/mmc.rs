//! Platform-neutral DesignWare MSHC polling/PIO primitives.
//!
//! The board layer owns discovery, clocks, resets, pinmux and power sequencing.
//! It may use [`MmioRegisters`] or provide another [`RegisterIo`] backend.
use api_v0::MmioRegion;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MmcError {
    InvalidParameter,
    RegisterOutOfRange,
    Timeout,
    ResponseTimeout,
    Response,
    ResponseCrc,
    DataTimeout,
    DataCrc,
    Fifo,
    HardwareLocked,
}

pub trait RegisterIo {
    fn read32(&mut self, offset : usize) -> Result<u32, MmcError>;
    fn write32(&mut self, offset : usize, value : u32) -> Result<(), MmcError>;
}

pub struct MmioRegisters {
    base : usize,
    size : usize,
}
impl MmioRegisters {
    /// # Safety
    /// The caller must ensure that the region is mapped device memory and is
    /// exclusively controlled through this instance while it is alive.
    pub unsafe fn new(region : MmioRegion) -> Self {
        Self { base : region.base,
               size : region.size }
    }
}
impl RegisterIo for MmioRegisters {
    fn read32(&mut self, offset : usize) -> Result<u32, MmcError> {
        if offset.checked_add(4)
                 .is_none_or(|end| end > self.size) ||
           offset % 4 != 0
        {
            return Err(MmcError::RegisterOutOfRange);
        }
        // SAFETY: guaranteed by `new`; the bounds and alignment are checked above.
        let address = self.base
                          .checked_add(offset)
                          .ok_or(MmcError::RegisterOutOfRange)?;
        Ok(unsafe { core::ptr::read_volatile(address as *const u32) })
    }
    fn write32(&mut self, offset : usize, value : u32) -> Result<(), MmcError> {
        if offset.checked_add(4)
                 .is_none_or(|end| end > self.size) ||
           offset % 4 != 0
        {
            return Err(MmcError::RegisterOutOfRange);
        }
        // SAFETY: guaranteed by `new`; the bounds and alignment are checked above.
        let address = self.base
                          .checked_add(offset)
                          .ok_or(MmcError::RegisterOutOfRange)?;
        unsafe { core::ptr::write_volatile(address as *mut u32, value) };
        Ok(())
    }
}

const CTRL : usize = 0x000;
const PWREN : usize = 0x004;
const CLKDIV : usize = 0x008;
const CLKSRC : usize = 0x00C;
const CLKENA : usize = 0x010;
const TMOUT : usize = 0x014;
const CTYPE : usize = 0x018;
const BLKSIZ : usize = 0x01C;
const BYTCNT : usize = 0x020;
const CMDARG : usize = 0x028;
const CMD : usize = 0x02C;
const RESP0 : usize = 0x030;
const RESP1 : usize = 0x034;
const RESP2 : usize = 0x038;
const RESP3 : usize = 0x03C;
const RINTSTS : usize = 0x044;
const STATUS : usize = 0x048;
const FIFOTH : usize = 0x04C;
const INTMASK : usize = 0x024;

const CTRL_RESET_ALL : u32 = 0b111;
/// DW MSHC CTRL 的 DMA/IDMAC 使能位（与 U-Boot `DWMCI_DMA_EN`/`DWMCI_IDMAC_EN`
/// 一致）：PIO 模式必须清除，否则数据被路由到 IDMAC 而非 FIFO。
const CTRL_DMA_ENABLE : u32 = 1 << 5;
const CTRL_IDMAC_ENABLE : u32 = 1 << 25;
const CMD_START : u32 = 1 << 31;
const CMD_USE_HOLD : u32 = 1 << 29;
const CMD_UPDATE_CLOCK : u32 = 1 << 21;
const CMD_WAIT_PREVIOUS_DATA : u32 = 1 << 13;
const CMD_SEND_INITIALIZATION : u32 = 1 << 15;
const CMD_DATA_EXPECTED : u32 = 1 << 9;
const CMD_CHECK_RESPONSE_CRC : u32 = 1 << 8;
const CMD_RESPONSE_EXPECTED : u32 = 1 << 6;
const CMD_RESPONSE_LONG : u32 = 1 << 7;
const INT_END_BIT : u32 = 1 << 15;
const INT_START_BIT : u32 = 1 << 13;
const INT_HARDWARE_LOCKED : u32 = 1 << 12;
const INT_FIFO_RUN : u32 = 1 << 11;
const INT_DATA_TIMEOUT : u32 = 1 << 9;
const INT_RESPONSE_TIMEOUT : u32 = 1 << 8;
const INT_DATA_CRC : u32 = 1 << 7;
const INT_RESPONSE_CRC : u32 = 1 << 6;
const INT_RX_READY : u32 = 1 << 5;
const INT_DATA_OVER : u32 = 1 << 3;
const INT_COMMAND_DONE : u32 = 1 << 2;
const INT_RESPONSE_ERROR : u32 = 1 << 1;
const INT_ALL : u32 = 0x1FFFF;

pub struct DwMmc<R> {
    registers : R,
    fifo_offset : usize,
    poll_limit : usize,
}

/// Calculate the 8-bit DesignWare divider and the resulting card clock.
/// Divider zero bypasses division; non-zero values produce input/(2*div).
pub fn clock_divider(input_hz : u32, target_hz : u32) -> Result<(u8, u32), MmcError> {
    if input_hz == 0 || target_hz == 0 {
        return Err(MmcError::InvalidParameter);
    }
    if target_hz >= input_hz {
        return Ok((0, input_hz));
    }
    let denominator = 2u64 * target_hz as u64;
    let divider = (input_hz as u64).div_ceil(denominator);
    if divider == 0 || divider > u8::MAX as u64 {
        return Err(MmcError::InvalidParameter);
    }
    let actual = input_hz / (2 * divider as u32);
    Ok((divider as u8, actual))
}

/// Encode the DesignWare CTYPE bus-width field without touching registers.
pub const fn bus_width_value(bus_width : u8) -> Result<u32, MmcError> {
    match bus_width {
        1 => Ok(0),
        4 => Ok(1),
        8 => Ok(1 << 16),
        _ => Err(MmcError::InvalidParameter),
    }
}

impl<R : RegisterIo> DwMmc<R> {
    /// 读数据失败时抓取关键寄存器，便于真机定位具体错误位。
    fn read_failure(&mut self, err : MmcError) -> MmcError {
        let rintsts = self.registers
                          .read32(RINTSTS)
                          .unwrap_or(0);
        let status = self.registers
                         .read32(STATUS)
                         .unwrap_or(0);
        let ctrl = self.registers
                       .read32(CTRL)
                       .unwrap_or(0);
        let resp0 = self.registers
                        .read32(RESP0)
                        .unwrap_or(0);
        log::error!("[dw-mmc] read_single_block failed err={err:?} \
                     rintsts={rintsts:#x} status={status:#x} ctrl={ctrl:#x} \
                     resp0={resp0:#x} fifo_offset={:#x}",
                    self.fifo_offset);
        err
    }

    pub fn probe(mut registers : R, poll_limit : usize) -> Result<Self, MmcError> {
        // FIFO 数据寄存器固定 0x200（与同板 U-Boot `DWMCI_DATA` 一致）。
        // 按 VERID 猜测 0x100 会在真机读数据时取错偏移 → FIFO 溢出。
        let fifo_offset = 0x200;
        Ok(Self { registers,
                  fifo_offset,
                  poll_limit : poll_limit.max(1) })
    }

    pub fn reset(&mut self) -> Result<(), MmcError> {
        self.registers
            .write32(CTRL, CTRL_RESET_ALL)?;
        for _ in 0..self.poll_limit {
            if self.registers
                   .read32(CTRL)? &
               CTRL_RESET_ALL ==
               0
            {
                return Ok(());
            }
        }
        Err(MmcError::Timeout)
    }

    fn update_clock(&mut self) -> Result<(), MmcError> {
        self.registers
            .write32(RINTSTS, INT_ALL)?;
        self.registers
            .write32(CMD,
                     CMD_START | CMD_USE_HOLD | CMD_WAIT_PREVIOUS_DATA | CMD_UPDATE_CLOCK)?;
        for _ in 0..self.poll_limit {
            let interrupts = self.registers
                                 .read32(RINTSTS)?;
            if let Err(err) = Self::check_errors(interrupts) {
                return Err(self.read_failure(err));
            }
            if self.registers
                   .read32(CMD)? &
               CMD_START ==
               0
            {
                if interrupts != 0 {
                    self.registers
                        .write32(RINTSTS, interrupts)?;
                }
                return Ok(());
            }
        }
        Err(MmcError::Timeout)
    }

    /// Configure the controller-internal card clock without exceeding target.
    pub fn configure_card_clock(&mut self,
                                input_hz : u32,
                                target_hz : u32)
                                -> Result<u32, MmcError> {
        let (divider, actual) = clock_divider(input_hz, target_hz)?;
        self.registers
            .write32(CLKENA, 0)?;
        self.update_clock()?;
        self.registers
            .write32(CLKDIV, divider as u32)?;
        self.registers
            .write32(CLKSRC, 0)?;
        self.update_clock()?;
        self.registers
            .write32(CLKENA, 1)?;
        self.update_clock()?;
        Ok(actual)
    }

    /// Conservative polling/PIO setup for SD identification mode.
    /// The upstream JH7110 AHB/CIU clocks, reset and pinmux must already be
    /// configured by a board layer; that prerequisite is UNVERIFIED.
    pub fn initialize_polling(&mut self,
                              input_hz : u32,
                              target_hz : u32,
                              fifo_depth : u32)
                              -> Result<u32, MmcError> {
        self.initialize_polling_with_bus_width(input_hz, target_hz, fifo_depth, 1)
    }

    /// Conservative polling/PIO setup with an explicit SD bus width.
    pub fn initialize_polling_with_bus_width(&mut self,
                                             input_hz : u32,
                                             target_hz : u32,
                                             fifo_depth : u32,
                                             bus_width : u8)
                                             -> Result<u32, MmcError> {
        if !(2..=4096).contains(&fifo_depth) {
            return Err(MmcError::InvalidParameter);
        }
        let ctype = bus_width_value(bus_width)?;
        self.reset()?;
        // 强制 PIO：清除固件可能遗留的 DMA/IDMAC 使能位。
        let ctrl = self.registers
                       .read32(CTRL)?;
        self.registers
            .write32(CTRL, ctrl & !(CTRL_DMA_ENABLE | CTRL_IDMAC_ENABLE))?;
        self.registers
            .write32(PWREN, 1)?;
        self.registers
            .write32(TMOUT, u32::MAX)?;
        self.registers
            .write32(CTYPE, ctype)?;
        self.registers
            .write32(INTMASK, 0)?;
        self.registers
            .write32(RINTSTS, INT_ALL)?;
        let receive_watermark = fifo_depth / 2 - 1;
        let transmit_watermark = fifo_depth / 2;
        self.registers
            .write32(FIFOTH,
                     (receive_watermark << 16) | transmit_watermark)?;
        self.configure_card_clock(input_hz, target_hz)
    }

    /// Execute a non-data command and return RESP0..RESP3.
    ///
    /// The caller selects response CRC behavior because OCR responses do not
    /// carry a valid CRC. CMD0 automatically requests the required 80 initial
    /// card clocks from the DesignWare controller. Board clock rate and power
    /// sequencing remain the caller's responsibility and are UNVERIFIED.
    pub fn execute_command(&mut self,
                           index : u8,
                           argument : u32,
                           response_expected : bool,
                           response_long : bool,
                           response_crc : bool)
                           -> Result<[u32; 4], MmcError> {
        if index > 63 || response_long && !response_expected {
            return Err(MmcError::InvalidParameter);
        }
        self.registers
            .write32(RINTSTS, INT_ALL)?;
        self.registers
            .write32(CMDARG, argument)?;
        let mut command = CMD_START | CMD_USE_HOLD | CMD_WAIT_PREVIOUS_DATA | index as u32;
        if index == 0 {
            command |= CMD_SEND_INITIALIZATION;
        }
        if response_expected {
            command |= CMD_RESPONSE_EXPECTED;
        }
        if response_long {
            command |= CMD_RESPONSE_LONG;
        }
        if response_crc {
            command |= CMD_CHECK_RESPONSE_CRC;
        }
        self.registers
            .write32(CMD, command)?;
        for _ in 0..self.poll_limit {
            let interrupts = self.registers
                                 .read32(RINTSTS)?;
            Self::check_errors(interrupts)?;
            if interrupts != 0 {
                self.registers
                    .write32(RINTSTS, interrupts)?;
            }
            if interrupts & INT_COMMAND_DONE != 0 {
                return Ok([self.registers
                               .read32(RESP0)?,
                           self.registers
                               .read32(RESP1)?,
                           self.registers
                               .read32(RESP2)?,
                           self.registers
                               .read32(RESP3)?]);
            }
        }
        Err(MmcError::Timeout)
    }

    /// Issues CMD17 and transfers exactly one 512-byte block through the FIFO.
    /// Clocking, card selection, addressing mode and voltage setup are the
    /// caller's responsibility.
    pub fn read_single_block(&mut self,
                             argument : u32,
                             output : &mut [u8])
                             -> Result<u32, MmcError> {
        if output.len() != 512 {
            return Err(MmcError::InvalidParameter);
        }
        self.registers
            .write32(RINTSTS, INT_ALL)?;
        self.registers
            .write32(BLKSIZ, 512)?;
        self.registers
            .write32(BYTCNT, 512)?;
        self.registers
            .write32(CMDARG, argument)?;
        self.registers
            .write32(CMD,
                     CMD_START |
                     CMD_USE_HOLD |
                     CMD_WAIT_PREVIOUS_DATA |
                     CMD_DATA_EXPECTED |
                     CMD_CHECK_RESPONSE_CRC |
                     CMD_RESPONSE_EXPECTED |
                     17)?;

        let mut bytes = 0;
        let mut data_over = false;
        for _ in 0..self.poll_limit {
            let interrupts = self.registers
                                 .read32(RINTSTS)?;
            if let Err(err) = Self::check_errors(interrupts) {
                return Err(self.read_failure(err));
            }
            data_over |= interrupts & INT_DATA_OVER != 0;

            let fifo_words = ((self.registers
                                   .read32(STATUS)? >>
                               17) &
                              0x1FFF) as usize;
            if fifo_words > 0 || interrupts & INT_RX_READY != 0 {
                for _ in 0..fifo_words.min((output.len() - bytes) / 4) {
                    let word = self.registers
                                   .read32(self.fifo_offset)?;
                    output[bytes..bytes + 4].copy_from_slice(&word.to_le_bytes());
                    bytes += 4;
                }
            }
            if interrupts != 0 {
                self.registers
                    .write32(RINTSTS, interrupts)?;
            }
            if data_over && bytes == output.len() {
                return self.registers
                           .read32(RESP0);
            }
        }
        Err(self.read_failure(MmcError::Timeout))
    }

    fn check_errors(interrupts : u32) -> Result<(), MmcError> {
        if interrupts & INT_RESPONSE_TIMEOUT != 0 {
            return Err(MmcError::ResponseTimeout);
        }
        if interrupts & INT_RESPONSE_CRC != 0 {
            return Err(MmcError::ResponseCrc);
        }
        if interrupts & INT_RESPONSE_ERROR != 0 {
            return Err(MmcError::Response);
        }
        if interrupts & INT_DATA_TIMEOUT != 0 {
            return Err(MmcError::DataTimeout);
        }
        if interrupts & INT_DATA_CRC != 0 {
            return Err(MmcError::DataCrc);
        }
        if interrupts & (INT_END_BIT | INT_START_BIT | INT_FIFO_RUN) != 0 {
            return Err(MmcError::Fifo);
        }
        if interrupts & INT_HARDWARE_LOCKED != 0 {
            return Err(MmcError::HardwareLocked);
        }
        Ok(())
    }

    #[cfg(test)]
    fn into_inner(self) -> R { self.registers }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{collections::VecDeque, vec, vec::Vec};

    struct MockRegisters {
        values : Vec<u32>,
        fifo : VecDeque<u32>,
        interrupts : u32,
        clear_reset : bool,
        clear_command_start : bool,
        clock_updates : usize,
    }
    impl MockRegisters {
        fn successful() -> Self {
            let fifo = (0..128).map(|word| 0xA500_0000 | word)
                               .collect();
            let mut values = vec![0; 0x204 / 4];
            values[RESP0 / 4] = 0x1234;
            Self { values,
                   fifo,
                   interrupts : INT_COMMAND_DONE | INT_RX_READY | INT_DATA_OVER,
                   clear_reset : true,
                   clear_command_start : true,
                   clock_updates : 0 }
        }
    }
    impl RegisterIo for MockRegisters {
        fn read32(&mut self, offset : usize) -> Result<u32, MmcError> {
            if offset == RINTSTS {
                return Ok(self.interrupts);
            }
            if offset == STATUS {
                return Ok((self.fifo.len() as u32) << 17);
            }
            if offset == 0x200 {
                return self.fifo
                           .pop_front()
                           .ok_or(MmcError::Fifo);
            }
            self.values
                .get(offset / 4)
                .copied()
                .ok_or(MmcError::RegisterOutOfRange)
        }
        fn write32(&mut self, offset : usize, value : u32) -> Result<(), MmcError> {
            if offset == RINTSTS {
                return Ok(());
            }
            let slot = self.values
                           .get_mut(offset / 4)
                           .ok_or(MmcError::RegisterOutOfRange)?;
            if offset == CMD && value & CMD_UPDATE_CLOCK != 0 {
                self.clock_updates += 1;
            }
            *slot = if offset == CTRL && self.clear_reset {
                0
            } else if offset == CMD && self.clear_command_start {
                value & !CMD_START
            } else {
                value
            };
            Ok(())
        }
    }

    #[test]
    fn reset_and_read_one_block_through_versioned_fifo() {
        let mut host = DwMmc::probe(MockRegisters::successful(), 4).unwrap();
        host.reset()
            .unwrap();
        let mut block = [0; 512];
        assert_eq!(host.read_single_block(7, &mut block),
                   Ok(0x1234));
        assert_eq!(&block[..4],
                   &0xA500_0000u32.to_le_bytes());
        assert_eq!(&block[508..],
                   &(0xA500_007Fu32).to_le_bytes());
        let registers = host.into_inner();
        assert_eq!(registers.values[BLKSIZ / 4], 512);
        assert_eq!(registers.values[BYTCNT / 4], 512);
        assert_eq!(registers.values[CMDARG / 4], 7);
        assert_eq!(registers.values[CMD / 4] & 0x3F, 17);
    }

    #[test]
    fn executes_initialization_and_long_response_commands() {
        let mut mock = MockRegisters::successful();
        mock.values[RESP1 / 4] = 2;
        mock.values[RESP2 / 4] = 3;
        mock.values[RESP3 / 4] = 4;
        let mut host = DwMmc::probe(mock, 2).unwrap();
        assert_eq!(host.execute_command(0, 0, false, false, false),
                   Ok([0x1234, 2, 3, 4]));
        let command0 = host.registers
                           .values[CMD / 4];
        assert_ne!(command0 & CMD_SEND_INITIALIZATION, 0);
        assert_eq!(host.execute_command(2, 0, true, true, true),
                   Ok([0x1234, 2, 3, 4]));
        let command2 = host.into_inner()
                           .values[CMD / 4];
        let response_flags = CMD_RESPONSE_EXPECTED | CMD_RESPONSE_LONG | CMD_CHECK_RESPONSE_CRC;
        assert_eq!(command2 & response_flags,
                   response_flags);
    }

    #[test]
    fn configures_bounded_identification_clock_and_fifo() {
        assert_eq!(bus_width_value(1), Ok(0));
        assert_eq!(bus_width_value(4), Ok(1));
        assert_eq!(bus_width_value(8), Ok(1 << 16));
        assert_eq!(bus_width_value(2), Err(MmcError::InvalidParameter));
        assert_eq!(clock_divider(50_000_000, 400_000),
                   Ok((63, 396_825)));
        assert_eq!(clock_divider(25_000_000, 50_000_000),
                   Ok((0, 25_000_000)));
        assert_eq!(clock_divider(1, 0),
                   Err(MmcError::InvalidParameter));
        assert_eq!(clock_divider(500_000_000, 1),
                   Err(MmcError::InvalidParameter));

        let mut host = DwMmc::probe(MockRegisters::successful(), 4).unwrap();
        assert_eq!(host.initialize_polling(50_000_000, 400_000, 32),
                   Ok(396_825));
        let registers = host.into_inner();
        assert_eq!(registers.clock_updates, 3);
        assert_eq!(registers.values[CLKDIV / 4], 63);
        assert_eq!(registers.values[CLKENA / 4], 1);
        assert_eq!(registers.values[CTYPE / 4], 0);
        assert_eq!(registers.values[INTMASK / 4], 0);
        assert_eq!(registers.values[FIFOTH / 4],
                   (15 << 16) | 16);

        let mut wide = DwMmc::probe(MockRegisters::successful(), 4).unwrap();
        assert_eq!(wide.initialize_polling_with_bus_width(50_000_000,
                                                          400_000,
                                                          32,
                                                          4),
                   Ok(396_825));
        assert_eq!(wide.into_inner().values[CTYPE / 4], 1);
    }

    #[test]
    fn clock_update_error_does_not_enable_clock() {
        let mut mock = MockRegisters::successful();
        mock.interrupts = INT_HARDWARE_LOCKED;
        let mut host = DwMmc::probe(mock, 2).unwrap();
        assert_eq!(host.configure_card_clock(50_000_000, 400_000),
                   Err(MmcError::HardwareLocked));
        let registers = host.into_inner();
        assert_eq!(registers.clock_updates, 1);
        assert_eq!(registers.values[CLKDIV / 4], 0);
        assert_eq!(registers.values[CLKENA / 4], 0);

        let mut mock = MockRegisters::successful();
        mock.interrupts = 0;
        mock.clear_command_start = false;
        let mut host = DwMmc::probe(mock, 2).unwrap();
        assert_eq!(host.configure_card_clock(50_000_000, 400_000),
                   Err(MmcError::Timeout));
        assert_eq!(host.into_inner()
                       .values[CLKENA / 4],
                   0);
    }

    #[test]
    fn reports_crc_and_bounded_timeout() {
        let mut crc = MockRegisters::successful();
        crc.interrupts = INT_DATA_CRC;
        let mut host = DwMmc::probe(crc, 2).unwrap();
        assert_eq!(host.read_single_block(0, &mut [0; 512]),
                   Err(MmcError::DataCrc));

        let mut timeout = MockRegisters::successful();
        timeout.interrupts = 0;
        timeout.fifo.clear();
        let mut host = DwMmc::probe(timeout, 2).unwrap();
        assert_eq!(host.read_single_block(0, &mut [0; 512]),
                   Err(MmcError::Timeout));
    }

    #[test]
    fn rejects_wrong_block_size_and_pins_fifo_to_0x200() {
        let mut mock = MockRegisters::successful();
        let mut host = DwMmc::probe(mock, 1).unwrap();
        assert_eq!(host.fifo_offset, 0x200);
        assert_eq!(host.read_single_block(0, &mut [0; 4]),
                   Err(MmcError::InvalidParameter));
    }
}

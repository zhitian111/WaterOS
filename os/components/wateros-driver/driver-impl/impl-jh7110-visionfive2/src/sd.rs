//! SD card protocol state machine and a conservative read-only block adapter.
//!
//! The protocol is testable without a board through [`SdTransport`]. The real
//! JH7110 transport is intentionally not activated yet: clock/reset, pinmux,
//! voltage and card-detect behavior remain UNVERIFIED on physical hardware.
use crate::mmc::{DwMmc, MmcError, RegisterIo};
use block::{BLOCK_SIZE, BlockDevice, DriverError, DriverResult, Lba};

const CMD_GO_IDLE : u8 = 0;
const CMD_ALL_SEND_CID : u8 = 2;
const CMD_SEND_RELATIVE_ADDRESS : u8 = 3;
const CMD_SELECT_CARD : u8 = 7;
const CMD_SEND_IF_COND : u8 = 8;
const CMD_SET_BLOCK_LENGTH : u8 = 16;
const CMD_APP : u8 = 55;
const ACMD_SD_SEND_OP_COND : u8 = 41;

const IF_COND_27_TO_36_V_AND_PATTERN : u32 = 0x1AA;
const OCR_VOLTAGE_27_TO_36_V : u32 = 0x00FF_8000;
const OCR_HIGH_CAPACITY_REQUEST : u32 = 1 << 30;
const OCR_CARD_POWERED_UP : u32 = 1 << 31;
const OCR_CARD_CAPACITY_STATUS : u32 = 1 << 30;
const R1_APP_COMMAND : u32 = 1 << 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseKind {
    None,
    Short,
    ShortNoCrc,
    Long,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandResponse {
    None,
    Short(u32),
    Long([u32; 4]),
}

/// Transport boundary between the SD protocol and a host controller.
pub trait SdTransport {
    fn command(&mut self,
               index : u8,
               argument : u32,
               response : ResponseKind)
               -> Result<CommandResponse, MmcError>;
    fn read_single_block(&mut self,
                         argument : u32,
                         output : &mut [u8; BLOCK_SIZE])
                         -> Result<(), MmcError>;
}

impl<R : RegisterIo> SdTransport for DwMmc<R> {
    fn command(&mut self,
               index : u8,
               argument : u32,
               response : ResponseKind)
               -> Result<CommandResponse, MmcError> {
        let (expected, long, crc) = match response {
            ResponseKind::None => (false, false, false),
            ResponseKind::Short => (true, false, true),
            ResponseKind::ShortNoCrc => (true, false, false),
            ResponseKind::Long => (true, true, true),
        };
        let words = self.execute_command(index, argument, expected, long, crc)?;
        Ok(match response {
            ResponseKind::None => CommandResponse::None,
            ResponseKind::Short | ResponseKind::ShortNoCrc => CommandResponse::Short(words[0]),
            ResponseKind::Long => CommandResponse::Long(words),
        })
    }

    fn read_single_block(&mut self,
                         argument : u32,
                         output : &mut [u8; BLOCK_SIZE])
                         -> Result<(), MmcError> {
        DwMmc::read_single_block(self, argument, output).map(|_| ())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardAddressing {
    Byte,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SdCardInfo {
    pub relative_address : u16,
    pub addressing : CardAddressing,
    /// Filled once CSD capacity parsing is implemented and hardware-validated.
    pub total_blocks : Option<u64>,
}

pub struct SdCard<T> {
    transport : T,
    info : SdCardInfo,
}

impl<T : SdTransport> SdCard<T> {
    /// Initialize a card at the caller-provided low-speed identification clock.
    /// `ocr_attempts` bounds ACMD41 polling and must be non-zero.
    pub fn initialize(mut transport : T, ocr_attempts : usize) -> Result<Self, MmcError> {
        if ocr_attempts == 0 {
            return Err(MmcError::InvalidParameter);
        }
        expect_none(transport.command(CMD_GO_IDLE, 0, ResponseKind::None)?)?;

        let version_two = match transport.command(CMD_SEND_IF_COND,
                                                  IF_COND_27_TO_36_V_AND_PATTERN,
                                                  ResponseKind::Short)
        {
            Ok(response) => {
                let value = expect_short(response)?;
                if value & 0xFFF != IF_COND_27_TO_36_V_AND_PATTERN {
                    return Err(MmcError::Response);
                }
                true
            },
            // A legacy v1 card is allowed not to understand CMD8.
            Err(MmcError::ResponseTimeout) => false,
            Err(error) => return Err(error),
        };

        let mut ocr = None;
        for _ in 0..ocr_attempts {
            let app_status = expect_short(transport.command(CMD_APP, 0, ResponseKind::Short)?)?;
            if app_status & R1_APP_COMMAND == 0 {
                return Err(MmcError::Response);
            }
            let argument = OCR_VOLTAGE_27_TO_36_V |
                           if version_two { OCR_HIGH_CAPACITY_REQUEST } else { 0 };
            let value = expect_short(transport.command(ACMD_SD_SEND_OP_COND,
                                                       argument,
                                                       ResponseKind::ShortNoCrc)?)?;
            if value & OCR_CARD_POWERED_UP != 0 {
                ocr = Some(value);
                break;
            }
        }
        let ocr = ocr.ok_or(MmcError::Timeout)?;
        if ocr & OCR_VOLTAGE_27_TO_36_V == 0 {
            return Err(MmcError::Response);
        }
        expect_long(transport.command(CMD_ALL_SEND_CID, 0, ResponseKind::Long)?)?;
        let relative_address = (expect_short(transport.command(CMD_SEND_RELATIVE_ADDRESS,
                                                               0,
                                                               ResponseKind::Short)?)? >>
                                16) as u16;
        if relative_address == 0 {
            return Err(MmcError::Response);
        }
        expect_short(transport.command(CMD_SELECT_CARD,
                                       (relative_address as u32) << 16,
                                       ResponseKind::Short)?)?;

        let addressing = if version_two && ocr & OCR_CARD_CAPACITY_STATUS != 0 {
            CardAddressing::Block
        } else {
            expect_short(transport.command(CMD_SET_BLOCK_LENGTH,
                                           BLOCK_SIZE as u32,
                                           ResponseKind::Short)?)?;
            CardAddressing::Byte
        };
        Ok(Self { transport,
                  info : SdCardInfo { relative_address,
                                      addressing,
                                      total_blocks : None } })
    }

    pub fn info(&self) -> SdCardInfo { self.info }
    pub fn into_transport(self) -> T { self.transport }

    fn command_argument(&self, lba : u64) -> Result<u32, MmcError> {
        let address = match self.info.addressing {
            CardAddressing::Block => lba,
            CardAddressing::Byte => lba.checked_mul(BLOCK_SIZE as u64)
                                              .ok_or(MmcError::InvalidParameter)?,
        };
        u32::try_from(address).map_err(|_| MmcError::InvalidParameter)
    }
}

impl<T : SdTransport + Send> BlockDevice for SdCard<T> {
    fn total_blocks(&self) -> Option<u64> { self.info.total_blocks }

    fn read_blocks(&mut self, start_block : Lba, output : &mut [u8]) -> DriverResult<()> {
        if output.len() % BLOCK_SIZE != 0 {
            return Err(DriverError::InvalidParam);
        }
        let blocks = output.len() / BLOCK_SIZE;
        for (offset, block) in output.chunks_exact_mut(BLOCK_SIZE).enumerate() {
            let lba = start_block.0
                                 .checked_add(offset as u64)
                                 .ok_or(DriverError::InvalidParam)?;
            if self.info.total_blocks.is_some_and(|total| lba >= total) {
                return Err(DriverError::InvalidParam);
            }
            let argument = self.command_argument(lba).map_err(map_error)?;
            let block : &mut [u8; BLOCK_SIZE] = block.try_into()
                                                        .map_err(|_| DriverError::InvalidParam)?;
            self.transport.read_single_block(argument, block).map_err(map_error)?;
        }
        debug_assert_eq!(blocks * BLOCK_SIZE, output.len());
        Ok(())
    }

    fn write_blocks(&mut self, _start_block : Lba, _input : &[u8]) -> DriverResult<()> {
        Err(DriverError::Unsupported)
    }
}

fn map_error(error : MmcError) -> DriverError {
    match error {
        MmcError::InvalidParameter | MmcError::RegisterOutOfRange => DriverError::InvalidParam,
        _ => DriverError::IoError,
    }
}
fn expect_none(response : CommandResponse) -> Result<(), MmcError> {
    match response {
        CommandResponse::None => Ok(()),
        _ => Err(MmcError::Response),
    }
}
fn expect_short(response : CommandResponse) -> Result<u32, MmcError> {
    match response {
        CommandResponse::Short(value) => Ok(value),
        _ => Err(MmcError::Response),
    }
}
fn expect_long(response : CommandResponse) -> Result<[u32; 4], MmcError> {
    match response {
        CommandResponse::Long(value) => Ok(value),
        _ => Err(MmcError::Response),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::{collections::VecDeque, vec, vec::Vec};

    #[derive(Debug)]
    struct ExpectedCommand {
        index : u8,
        argument : u32,
        kind : ResponseKind,
        result : Result<CommandResponse, MmcError>,
    }
    struct ScriptedCard {
        commands : VecDeque<ExpectedCommand>,
        reads : Vec<u32>,
    }
    impl ScriptedCard {
        fn new(commands : Vec<ExpectedCommand>) -> Self {
            Self { commands: commands.into(), reads: Vec::new() }
        }
    }
    impl SdTransport for ScriptedCard {
        fn command(&mut self,
                   index : u8,
                   argument : u32,
                   kind : ResponseKind)
                   -> Result<CommandResponse, MmcError> {
            let expected = self.commands.pop_front().expect("unexpected command");
            assert_eq!((index, argument, kind),
                       (expected.index, expected.argument, expected.kind));
            expected.result
        }
        fn read_single_block(&mut self,
                             argument : u32,
                             output : &mut [u8; BLOCK_SIZE])
                             -> Result<(), MmcError> {
            self.reads.push(argument);
            for (index, byte) in output.iter_mut().enumerate() {
                *byte = argument.wrapping_add(index as u32) as u8;
            }
            Ok(())
        }
    }

    fn command(index : u8,
               argument : u32,
               kind : ResponseKind,
               response : CommandResponse)
               -> ExpectedCommand {
        ExpectedCommand { index, argument, kind, result: Ok(response) }
    }
    fn common_prefix(cmd8 : Result<CommandResponse, MmcError>, acmd41_argument : u32,
                     ocr : u32) -> Vec<ExpectedCommand> {
        vec![command(0, 0, ResponseKind::None, CommandResponse::None),
             ExpectedCommand { index: 8, argument: 0x1AA, kind: ResponseKind::Short,
                               result: cmd8 },
             command(55, 0, ResponseKind::Short,
                     CommandResponse::Short(R1_APP_COMMAND)),
             command(41, acmd41_argument, ResponseKind::ShortNoCrc,
                     CommandResponse::Short(ocr)),
             command(2, 0, ResponseKind::Long, CommandResponse::Long([1, 2, 3, 4])),
             command(3, 0, ResponseKind::Short, CommandResponse::Short(0x1234_0000)),
             command(7, 0x1234_0000, ResponseKind::Short, CommandResponse::Short(0))]
    }

    #[test]
    fn initializes_sdhc_and_uses_block_addressing() {
        let commands = common_prefix(Ok(CommandResponse::Short(0x1AA)),
                                     OCR_VOLTAGE_27_TO_36_V | OCR_HIGH_CAPACITY_REQUEST,
                                     OCR_CARD_POWERED_UP | OCR_CARD_CAPACITY_STATUS |
                                     OCR_VOLTAGE_27_TO_36_V);
        let mut card = SdCard::initialize(ScriptedCard::new(commands), 2).unwrap();
        assert_eq!(card.info().addressing, CardAddressing::Block);
        let mut data = [0; BLOCK_SIZE * 2];
        card.read_blocks(Lba(7), &mut data).unwrap();
        assert_eq!(&data[..4], &[7, 8, 9, 10]);
        assert_eq!(&data[BLOCK_SIZE..BLOCK_SIZE + 4], &[8, 9, 10, 11]);
        let transport = card.into_transport();
        assert_eq!(transport.reads, vec![7, 8]);
        assert!(transport.commands.is_empty());
    }

    #[test]
    fn initializes_legacy_card_and_uses_byte_addressing() {
        let mut commands = common_prefix(Err(MmcError::ResponseTimeout),
                                         OCR_VOLTAGE_27_TO_36_V,
                                         OCR_CARD_POWERED_UP | OCR_VOLTAGE_27_TO_36_V);
        commands.push(command(16, 512, ResponseKind::Short, CommandResponse::Short(0)));
        let mut card = SdCard::initialize(ScriptedCard::new(commands), 1).unwrap();
        assert_eq!(card.info().addressing, CardAddressing::Byte);
        card.read_blocks(Lba(3), &mut [0; BLOCK_SIZE]).unwrap();
        assert_eq!(card.read_blocks(Lba(u32::MAX as u64 / 512 + 1),
                                    &mut [0; BLOCK_SIZE]),
                   Err(DriverError::InvalidParam));
        assert_eq!(card.into_transport().reads, vec![1536]);
    }

    #[test]
    fn bounds_ocr_polling_and_rejects_bad_protocol() {
        let waiting = vec![command(0, 0, ResponseKind::None, CommandResponse::None),
                           command(8, 0x1AA, ResponseKind::Short,
                                   CommandResponse::Short(0x1AA)),
                           command(55, 0, ResponseKind::Short,
                                   CommandResponse::Short(R1_APP_COMMAND)),
                           command(41, OCR_VOLTAGE_27_TO_36_V | OCR_HIGH_CAPACITY_REQUEST,
                                   ResponseKind::ShortNoCrc, CommandResponse::Short(0))];
        assert!(matches!(SdCard::initialize(ScriptedCard::new(waiting), 1),
                         Err(MmcError::Timeout)));

        let bad_cmd8 = vec![command(0, 0, ResponseKind::None, CommandResponse::None),
                            command(8, 0x1AA, ResponseKind::Short,
                                    CommandResponse::Short(0x1AB))];
        assert!(matches!(SdCard::initialize(ScriptedCard::new(bad_cmd8), 1),
                         Err(MmcError::Response)));
    }

    #[test]
    fn rejects_address_overflow_bad_buffers_and_writes() {
        let commands = common_prefix(Ok(CommandResponse::Short(0x1AA)),
                                     OCR_VOLTAGE_27_TO_36_V | OCR_HIGH_CAPACITY_REQUEST,
                                     OCR_CARD_POWERED_UP | OCR_CARD_CAPACITY_STATUS |
                                     OCR_VOLTAGE_27_TO_36_V);
        let mut card = SdCard::initialize(ScriptedCard::new(commands), 1).unwrap();
        assert_eq!(card.read_blocks(Lba(u32::MAX as u64 + 1), &mut [0; BLOCK_SIZE]),
                   Err(DriverError::InvalidParam));
        assert_eq!(card.read_blocks(Lba(0), &mut [0; 3]), Err(DriverError::InvalidParam));
        assert_eq!(card.write_blocks(Lba(0), &[0; BLOCK_SIZE]), Err(DriverError::Unsupported));
        assert!(card.into_transport().reads.is_empty());
    }
}

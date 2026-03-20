use api_v0::console::{FirmwareConsole, FirmwareConsoleError, FirmwareConsoleResult};
#[allow(unused)]
use sbi::{console_write, console_write_byte};
pub struct OpenSBIConsole;
impl FirmwareConsole for OpenSBIConsole {
    #[inline]
    fn firmware_console_write_a_byte(byte : u8) -> FirmwareConsoleResult<()> {
        if console_write_byte(byte).is_ok() {
            Ok(())
        } else {
            Err(FirmwareConsoleError::WriteFailure)
        }
    }
}

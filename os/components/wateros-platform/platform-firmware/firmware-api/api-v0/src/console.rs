use core::result::Result;
#[derive(Debug)]
pub enum FirmwareConsoleError {
    Unavailable,
    Unsupported,
    WriteFailure,
    BufferFailure,
}
pub type FirmwareConsoleResult<T> = Result<T, FirmwareConsoleError>;
pub trait FirmwareConsole {
    #[inline]
    fn is_available() -> bool { true }
    #[inline]
    #[allow(unused_variables)]
    fn firmware_console_write_a_byte(byte : u8) -> FirmwareConsoleResult<()> {
        Err(FirmwareConsoleError::Unsupported)
    }
    #[inline]
    fn firmware_console_write_a_buffer(bytes : &[u8]) -> FirmwareConsoleResult<()> {
        if !Self::is_available() {
            Err(FirmwareConsoleError::Unavailable)
        } else {
            for &byte in bytes {
                Self::firmware_console_write_a_byte(byte)?
            }
            Ok(())
        }
    }
    #[inline]
    fn firmware_console_flush() -> FirmwareConsoleResult<()> {
        Err(FirmwareConsoleError::BufferFailure)
    }
}

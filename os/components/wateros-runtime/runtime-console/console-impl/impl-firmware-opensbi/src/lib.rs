#![no_std]

#[inline]
#[allow(unused)]
pub fn firmware_write_a_byte(byte : u8) { firmware::console::console_write_a_byte(byte).unwrap(); }

#[inline]
#[allow(unused)]
pub fn firmware_write_a_buffer(bytes : &[u8]) {
    firmware::console::console_write_a_buffer(&bytes).unwrap();
}

use core::fmt::{self, Write};

#[derive(Default)]
pub struct FirmwareConsoleHandle;
impl Write for FirmwareConsoleHandle {
    #[inline]
    fn write_str(&mut self, s : &str) -> fmt::Result {
        firmware_write_a_buffer(s.as_bytes());
        Ok(())
    }
}
impl api_v0::Console for FirmwareConsoleHandle {}

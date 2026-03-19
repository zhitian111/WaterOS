#![no_std]
use api_v0::console::FirmwareConsole;
use api_v0::reset::FirmwareReset;
use api_v0::timer::FirmwareTimer;
pub struct DummyReset;
impl FirmwareReset for DummyReset {
    fn is_available() -> bool {
        unimplemented!()
    }
    #[allow(unused_variables)]
    fn firmware_reset(
        reset_type: api_v0::reset::FirmwareResetType,
        reset_reason: api_v0::reset::FirmwareResetReason,
    ) -> api_v0::reset::FirmwareResetResult<()> {
        unimplemented!()
    }
    #[allow(unused_variables)]
    fn firmware_reboot(
        reset_reason: api_v0::reset::FirmwareResetReason,
    ) -> api_v0::reset::FirmwareResetResult<()> {
        unimplemented!()
    }
    #[allow(unused_variables)]
    fn firmware_shutdown(
        reset_reason: api_v0::reset::FirmwareResetReason,
    ) -> api_v0::reset::FirmwareResetResult<()> {
        unimplemented!()
    }
}
pub struct DummyTimer;
impl FirmwareTimer for DummyTimer {
    fn is_available() -> bool {
        unimplemented!()
    }
    #[allow(unused_variables)]
    fn firmware_set_timer(
        time: api_v0::timer::FirmwareTimerDeadline,
    ) -> api_v0::timer::FirmwareTimerResult<()> {
        unimplemented!()
    }
}
pub struct DummyConsole;
impl FirmwareConsole for DummyConsole {
    fn is_available() -> bool {
        unimplemented!()
    }
    fn firmware_console_flush() -> api_v0::console::FirmwareConsoleResult<()> {
        unimplemented!()
    }
    #[allow(unused_variables)]
    fn firmware_console_write_a_byte(byte: u8) -> api_v0::console::FirmwareConsoleResult<()> {
        unimplemented!()
    }
    #[allow(unused_variables)]
    fn firmware_console_write_a_buffer(bytes: &[u8]) -> api_v0::console::FirmwareConsoleResult<()> {
        unimplemented!()
    }
}

#![no_std]

//! **占位**固件实现：满足 trait 以通过编译与单测；方法体为 `unimplemented!()`。
//!
//! 与 `impl-opensbi` 由 `wateros-platform-firmware` 的 feature 二选一或并存占位；
//! 不得用于期望真实 SBI 行为的运行环境。

use api_v0::console::FirmwareConsole;
use api_v0::reset::FirmwareReset;
use api_v0::timer::FirmwareTimer;

/// 占位复位实现。
pub struct DummyReset;
impl FirmwareReset for DummyReset {
    fn is_available() -> bool { unimplemented!() }
    #[allow(unused_variables)]
    fn firmware_reset(reset_type : api_v0::reset::FirmwareResetType,
                      reset_reason : api_v0::reset::FirmwareResetReason)
                      -> api_v0::reset::FirmwareResetResult<()> {
        unimplemented!()
    }
    #[allow(unused_variables)]
    fn firmware_reboot(reset_reason : api_v0::reset::FirmwareResetReason)
                       -> api_v0::reset::FirmwareResetResult<()> {
        unimplemented!()
    }
    #[allow(unused_variables)]
    fn firmware_shutdown(reset_reason : api_v0::reset::FirmwareResetReason)
                         -> api_v0::reset::FirmwareResetResult<()> {
        unimplemented!()
    }
}

/// 占位定时器实现。
pub struct DummyTimer;
impl FirmwareTimer for DummyTimer {
    fn is_available() -> bool { unimplemented!() }
    #[allow(unused_variables)]
    fn firmware_set_timer(time : api_v0::timer::FirmwareTimerDeadline)
                          -> api_v0::timer::FirmwareTimerResult<()> {
        unimplemented!()
    }
}

/// 占位控制台实现。
pub struct DummyConsole;
impl FirmwareConsole for DummyConsole {
    fn is_available() -> bool { unimplemented!() }
    fn firmware_console_flush() -> api_v0::console::FirmwareConsoleResult<()> { unimplemented!() }
    #[allow(unused_variables)]
    fn firmware_console_write_a_byte(byte : u8) -> api_v0::console::FirmwareConsoleResult<()> {
        unimplemented!()
    }
    #[allow(unused_variables)]
    fn firmware_console_write_a_buffer(bytes : &[u8])
                                       -> api_v0::console::FirmwareConsoleResult<()> {
        unimplemented!()
    }
}

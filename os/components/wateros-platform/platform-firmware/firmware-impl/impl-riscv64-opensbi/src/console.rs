//! SBI 控制台扩展：字节写入路径（依赖 OpenSBI / QEMU 提供的 debug console）。
//!
//! 缓冲写入使用 `firmware-api` 默认循环；本实现仅覆盖单字节路径。

use api_v0::console::{FirmwareConsole, FirmwareConsoleError, FirmwareConsoleResult};
#[allow(unused)]
use sbi::{console_write, console_write_byte};
/// OpenSBI 控制台后端。
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

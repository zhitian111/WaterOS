#![no_std]

//! **固件 / SBI 层**：在 `firmware-api-v0` 上选择 `impl-opensbi`、`impl-dummy` 等，
//! 向内核暴露经固件 ABI 的能力（控制台、下次定时器、系统复位等）。
//!
//! ## 与 `wateros-platform-arch` 的边界
//! - 本 crate **不**直接操作与 ISA 规范强绑定的 CSR 细节作为对外 API；对外是
//!   “能写字节到固件控制台”“能请求固件在绝对时间触发中断”等**契约**。
//! - 具体实现通过 SBI 或其它固件接口完成；`arch` 层负责 `mtime` 读数、中断屏蔽等
//!   CPU 原语。二者由 `wateros-platform` 根 crate 组合（例如定时器组合模块）。

#[cfg(feature = "api-v0")]
/// 经固件或 SBI 的控制台输出（非 MMIO UART 驱动）。
pub mod console {
    pub use api_v0::console::{FirmwareConsole, FirmwareConsoleError, FirmwareConsoleResult};
    #[inline]
    #[allow(unused)]
    fn firmware_console_flush<Api : FirmwareConsole>() -> FirmwareConsoleResult<()> {
        Api::firmware_console_flush()
    }
    #[inline]
    #[allow(unused)]
    fn firmware_console_write_a_byte<Api : FirmwareConsole>(byte : u8)
                                                            -> FirmwareConsoleResult<()> {
        Api::firmware_console_write_a_byte(byte)
    }
    #[inline]
    #[allow(unused)]
    fn firmware_console_write_a_buffer<Api : FirmwareConsole>(bytes : &[u8])
                                                              -> FirmwareConsoleResult<()> {
        Api::firmware_console_write_a_buffer(bytes)
    }
    #[cfg(feature = "impl-dummy")]
    pub use impl_dummy::DummyConsole as FirmwareConsoleImpl;
    #[cfg(feature = "impl-qemu-loongarch64-uart16550")]
    pub use impl_qemu_loongarch64_uart16550::QemuLoongArch64Uart16550Console as FirmwareConsoleImpl;
    #[cfg(feature = "impl-riscv64-opensbi")]
    pub use impl_riscv64_opensbi::console::OpenSBIConsole as FirmwareConsoleImpl;
    #[inline]
    #[allow(unused)]
    pub fn console_flush() -> FirmwareConsoleResult<()> {
        FirmwareConsoleImpl::firmware_console_flush()
    }
    #[inline]
    #[allow(unused)]
    pub fn console_write_a_byte(byte : u8) -> FirmwareConsoleResult<()> {
        FirmwareConsoleImpl::firmware_console_write_a_byte(byte)
    }
    #[inline]
    #[allow(unused)]
    pub fn console_write_a_buffer(bytes : &[u8]) -> FirmwareConsoleResult<()> {
        FirmwareConsoleImpl::firmware_console_write_a_buffer(bytes)
    }
}

#[cfg(feature = "api-v0")]
/// 经 SBI 等的**绝对时刻**定时器编程（deadline 通常与 arch `time` tick 同源约定）。
pub mod timer {
    pub use api_v0::timer::{
        FirmwareTimer, FirmwareTimerDeadline, FirmwareTimerError, FirmwareTimerResult,
    };
    #[inline]
    #[allow(unused)]
    fn firmware_set_timer<Api : FirmwareTimer>(time : FirmwareTimerDeadline)
                                               -> FirmwareTimerResult<()> {
        Api::firmware_set_timer(time)
    }
    #[cfg(feature = "impl-dummy")]
    pub use impl_dummy::DummyTimer as FirmwareTimerImpl;
    #[cfg(feature = "impl-qemu-loongarch64-uart16550")]
    pub use impl_qemu_loongarch64_uart16550::QemuLoongArch64Timer as FirmwareTimerImpl;
    #[cfg(feature = "impl-riscv64-opensbi")]
    pub use impl_riscv64_opensbi::timer::OpenSBITimer as FirmwareTimerImpl;
    pub fn set_timer(time : FirmwareTimerDeadline) -> FirmwareTimerResult<()> {
        FirmwareTimerImpl::firmware_set_timer(time)
    }
}

#[cfg(feature = "api-v0")]
/// 关机、冷/热重启等系统复位请求（固件执行，非直接写设备复位寄存器）。
pub mod reset {
    pub use api_v0::reset::{
        FirmwareReset, FirmwareResetError, FirmwareResetReason, FirmwareResetResult,
        FirmwareResetType,
    };
    #[inline]
    #[allow(unused)]
    fn firmware_reset<Api : FirmwareReset>(reset_type : FirmwareResetType,
                                           reset_reason : FirmwareResetReason)
                                           -> FirmwareResetResult<()> {
        Api::firmware_reset(reset_type, reset_reason)
    }
    #[inline]
    #[allow(unused)]
    fn firmware_reboot<Api : FirmwareReset>(reset_reason : FirmwareResetReason)
                                            -> FirmwareResetResult<()> {
        Api::firmware_reboot(reset_reason)
    }
    #[inline]
    #[allow(unused)]
    fn firmware_shutdown<Api : FirmwareReset>(reset_reason : FirmwareResetReason)
                                              -> FirmwareResetResult<()> {
        Api::firmware_shutdown(reset_reason)
    }
    #[cfg(feature = "impl-dummy")]
    use impl_dummy::DummyReset as FirmwareResetImpl;
    #[cfg(feature = "impl-qemu-loongarch64-uart16550")]
    use impl_qemu_loongarch64_uart16550::QemuLoongArch64DummyReset as FirmwareResetImpl;
    #[cfg(feature = "impl-riscv64-opensbi")]
    use impl_riscv64_opensbi::reset::OpenSBIReset as FirmwareResetImpl;
    #[inline]
    #[allow(unused)]
    pub fn reset(reset_type : FirmwareResetType,
                 reset_reason : FirmwareResetReason)
                 -> FirmwareResetResult<()> {
        firmware_reset::<FirmwareResetImpl>(reset_type, reset_reason)
    }
    #[inline]
    #[allow(unused)]
    pub fn reboot(reset_reason : FirmwareResetReason) -> FirmwareResetResult<()> {
        firmware_reboot::<FirmwareResetImpl>(reset_reason)
    }
    #[inline]
    #[allow(unused)]
    pub fn shutdown(reset_reason : FirmwareResetReason) -> FirmwareResetResult<()> {
        firmware_shutdown::<FirmwareResetImpl>(reset_reason)
    }
}

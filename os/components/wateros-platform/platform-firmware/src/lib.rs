#![no_std]
#[cfg(feature = "api-v0")]
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
    #[cfg(feature = "impl-riscv64-opensbi")]
    pub use impl_riscv64_opensbi::console::OpenSBIConsole as FirmwareConsoleImpl;
    #[cfg(feature = "impl-qemu-loongarch64-uart16550")]
    pub use impl_qemu_loongarch64_uart16550::QemuLoongArch64Uart16550Console as FirmwareConsoleImpl;
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
    #[cfg(feature = "impl-riscv64-opensbi")]
    pub use impl_riscv64_opensbi::timer::OpenSBITimer as FirmwareTimerImpl;
    #[cfg(feature = "impl-qemu-loongarch64-uart16550")]
    pub use impl_qemu_loongarch64_uart16550::QemuLoongArch64DummyTimer as FirmwareTimerImpl;
    pub fn set_timer(time : FirmwareTimerDeadline) -> FirmwareTimerResult<()> {
        FirmwareTimerImpl::firmware_set_timer(time)
    }
}
#[cfg(feature = "api-v0")]
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
    #[cfg(feature = "impl-riscv64-opensbi")]
    use impl_riscv64_opensbi::reset::OpenSBIReset as FirmwareResetImpl;
    #[cfg(feature = "impl-qemu-loongarch64-uart16550")]
    use impl_qemu_loongarch64_uart16550::QemuLoongArch64DummyReset as FirmwareResetImpl;
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

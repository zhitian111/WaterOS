#![no_std]

pub fn panic_handler(_panic_info: &core::panic::PanicInfo) -> ! {
    use firmware::reset::{FirmwareResetReason, shutdown};
    while let Err(_e) = shutdown(FirmwareResetReason::SystemFailure) {}
    loop {}
}

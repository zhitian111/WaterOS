#![no_std]
pub fn panic_handler(_panic_info : &core::panic::PanicInfo) -> ! {
    use console::{println, AnsiColor};
    if let Some(location) = _panic_info.location() {
        println!("{}[WaterOS]{}{}    [{}] Panicked at {}:{}.  {}{}",
                 AnsiColor::Cyan,
                 AnsiColor::Clear,
                 AnsiColor::Red,
                 "PANIC",
                 location.file(),
                 location.line(),
                 _panic_info.message(),
                 AnsiColor::Clear);
    } else {
        println!("{}[WaterOS]{}{}    [{}] Panicked.   {}{}",
                 AnsiColor::Cyan,
                 AnsiColor::Clear,
                 AnsiColor::Red,
                 "PANIC",
                 _panic_info.message(),
                 AnsiColor::Clear);
    }

    use firmware::reset::{shutdown, FirmwareResetReason};
    while let Err(_e) = shutdown(FirmwareResetReason::SystemFailure) {}
    loop {}
}

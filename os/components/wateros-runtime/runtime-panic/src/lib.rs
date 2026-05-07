#![no_std]
//! 内核 panic 入口：打印位置与消息后请求固件关机并死循环兜底。

/// `#[panic_handler]` 使用的实现：输出到控制台后调用 `firmware::reset::shutdown`。
///
/// **当前行为**：若无法取得 `location()`，则仅打印消息行；关机失败时在 `loop {}` 中挂起。
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

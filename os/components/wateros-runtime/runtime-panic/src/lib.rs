#![no_std]
//! 内核 panic 入口：打印位置与消息后请求平台关机并死循环兜底。
//!
//! **边界**：依赖 `runtime-console` 与平台 reset 门面；控制台未就绪时仍尽力格式化输出（可能无显示）。

/// `#[panic_handler]` 使用的实现：输出到控制台后调用 `platform::reset::shutdown`。
///
/// **当前行为**：若无法取得 `location()`，则仅打印消息行；关机失败时在 `loop {}` 中挂起。
pub fn panic_handler(_panic_info : &core::panic::PanicInfo) -> ! {
    use console::{println, AnsiColor};
    // 优先带源码位置，便于现场对照符号与行号。
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
    let _ = platform::console::console_flush();

    use platform::reset::{shutdown, PlatformResetReason};
    // 关机可能因固件状态返回 Err；持续重试直至成功，避免误返回用户态。
    while let Err(_e) = shutdown(PlatformResetReason::SystemFailure) {}
    // 若关机实现存在但仍未终止执行，挂起 CPU；`!` 返回类型保证路径穷尽。
    loop {}
}

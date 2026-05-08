#![no_std]
//! 控制台输出抽象与便捷 API：基于 `api_v0::Console` 的类型参数化输出，并在 feature 下绑定具体 `ConsoleHandle`。
//!
//! - 带泛型的 `print` / `prints` 供库代码在已知 `Console` 实现时调用。
//! - `print!` / `println!` 宏默认使用本 crate 选中的 `ConsoleHandle`（dummy 或 OpenSBI 固件）。
//! - `write_raw_bytes` 面向非 UTF-8 或 syscall 路径；未启用 `impl-firmware-opensbi` 时为吞掉输出的占位行为。
//!
//! **平台假设**：ANSI 转义序列依赖接收端（串口终端或 QEMU）对 SGR 的支持；固件路径下与 OpenSBI 控制台扩展一致。

use core::fmt;

/// 终端 ANSI 颜色前缀，用于在 `println!` 等输出中高亮级别或横幅。
pub enum AnsiColor {
    Red,
    Green,
    Yellow,
    Blue,
    Purple,
    Cyan,
    White,
    Clear,
}

// 输出标准 SGR 转义序列（`\x1B[` … `m`）；与 `logger` 等模块的着色约定一致。
impl fmt::Display for AnsiColor {
    #[inline]
    fn fmt(&self, f : &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Clear => f.write_str("\x1B[0m"),
            Self::Red => f.write_str("\x1B[31m"),
            Self::Green => f.write_str("\x1B[32m"),
            Self::Yellow => f.write_str("\x1B[33m"),
            Self::Blue => f.write_str("\x1B[34m"),
            Self::Purple => f.write_str("\x1B[35m"),
            Self::Cyan => f.write_str("\x1B[36m"),
            Self::White => f.write_str("\x1B[37m"),
        }
    }
}

use api_v0::Console;

/// 使用类型 `C` 的 [`Console`] 实现格式化输出；失败时 `unwrap`（内核早期路径约定为不可恢复错误）。
#[inline]
pub fn print<C : Console>(args : fmt::Arguments) {
    let mut c = C::default();
    c.write_fmt(args)
     .unwrap();
}
/// 使用类型 `C` 的 [`Console`] 实现写入整段 UTF-8 文本。
#[inline]
pub fn prints<C : Console>(str : &str) {
    let mut c = C::default();
    c.write_str(str)
     .unwrap();
}

/// 将任意字节写入固件控制台（不要求 UTF-8）；供 `write` 系统调用等路径使用。
///
/// **契约**：无固件后端时静默丢弃，避免在无串口构建中引入链接依赖；有后端时按字节原样下发。
#[inline]
pub fn write_raw_bytes(bytes : &[u8]) {
    #[cfg(feature = "impl-firmware-opensbi")]
    impl_firmware_opensbi::firmware_write_a_buffer(bytes);
    // 未链 OpenSBI 控制台实现时保持无操作，便于仅编译/单测场景。
    #[cfg(not(feature = "impl-firmware-opensbi"))]
    {
        let _ = bytes;
    }
}

/// 当前 feature 选中的默认控制台句柄类型，供 `print!` / `println!` 使用。
// `impl-firmware-console` 与 `impl-firmware-opensbi` 为历史/别名关系，均指向同一固件实现 crate。
#[cfg(feature = "impl-dummy")]
pub use impl_dummy::DummyConsoleHandle as ConsoleHandle;
#[cfg(any(feature = "impl-firmware-console", feature = "impl-firmware-opensbi"))]
pub use impl_firmware_opensbi::FirmwareConsoleHandle as ConsoleHandle;

/// 使用 [`ConsoleHandle`] 的 `print!` 风格宏，等价于 `print::<ConsoleHandle>(format_args!(...))`。
#[macro_export]
macro_rules! print {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::print::<$crate::ConsoleHandle>(format_args!($fmt $(,$($arg)+)?));
    }
}
/// 使用 [`ConsoleHandle`] 的 `println!` 风格宏，自动追加换行。
#[macro_export]
macro_rules! println {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::print::<$crate::ConsoleHandle>(format_args!(concat!($fmt, "\n") $(,$($arg)+)?));
    }
}

/// 在控制台打印 WaterOS ASCII 横幅（调试用）；依赖已初始化的控制台后端。
///
/// 行尾使用 `\n\r` 以兼容部分串口/固件对换行的期望；若后端自行规范换行可再调整。
pub fn show_logo() {
    print!("{}", AnsiColor::Cyan);
    print!("██╗    ██╗ █████╗ ████████╗███████╗██████╗      ██████╗ ███████╗\n\r");
    print!("██║    ██║██╔══██╗╚══██╔══╝██╔════╝██╔══██╗    ██╔═══██╗██╔════╝\n\r");
    print!("██║ █╗ ██║███████║   ██║   █████╗  ██████╔╝    ██║   ██║███████╗\n\r");
    print!("██║███╗██║██╔══██║   ██║   ██╔══╝  ██╔══██╗    ██║   ██║╚════██║\n\r");
    print!("╚███╔███╔╝██║  ██║   ██║   ███████╗██║  ██║    ╚██████╔╝███████║\n\r");
    print!(" ╚══╝╚══╝ ╚═╝  ╚═╝   ╚═╝   ╚══════╝╚═╝  ╚═╝     ╚═════╝ ╚══════╝\n\r");
    print!("{}", AnsiColor::Clear);
}

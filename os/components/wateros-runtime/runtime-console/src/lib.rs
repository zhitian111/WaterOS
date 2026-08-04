#![no_std]
//! 控制台输出抽象与统一写入入口。
//!
//! `Console` trait 仅用于约束具体后端句柄；运行期调用方应使用 [`write_fmt`]、
//! [`write_str`] 或 [`write_raw_bytes`]，而不是自行选择后端类型。
//!
//! - `print!` / `println!` 宏调用统一 [`write_fmt`] 路径。
//! - `write_raw_bytes` 面向非 UTF-8 或 syscall 路径；未启用平台控制台时为吞掉输出的占位行为。
//!
//! **平台假设**：ANSI 转义序列依赖接收端（串口终端或 QEMU）对 SGR 的支持。
//!
//! OUTPUT_SYNC: backend 只负责设备写入，跨 CPU 的输出锁由 `platform::console` 提供；
//! runtime 绝不能再持有 scheduler、VFS 或 allocator 锁后调用输出。

use core::fmt;

#[cfg(all(feature = "impl-dummy", feature = "impl-platform-console"))]
compile_error!("enable only one runtime-console implementation");

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

/// 将格式化参数作为一条完整输出交给当前控制台后端。
///
/// RUNTIME_OUTPUT: platform backend 会在 platform console 的跨 CPU 锁中完成格式化；
/// 因此不能将 `fmt::Arguments` 拆成多次 `write_str`，否则不同 CPU 的日志仍可能交错。
/// 无 runtime console feature 时输出被显式丢弃，仅允许最小依赖构建使用。
#[inline]
pub fn write_fmt(args : fmt::Arguments<'_>) {
    #[cfg(feature = "impl-platform-console")]
    {
        impl_platform_console::platform_console_write_fmt(args);
        return;
    }
    #[cfg(feature = "impl-dummy")]
    {
        use core::fmt::Write;
        let mut console = ConsoleHandle::default();
        let _ = console.write_fmt(args);
        return;
    }
    #[cfg(not(any(feature = "impl-platform-console", feature = "impl-dummy")))]
    {
        let _ = args;
    }
}

/// 写入一段 UTF-8 文本；与 [`write_fmt`] 使用相同的整段原子性边界。
#[inline]
pub fn write_str(text : &str) {
    #[cfg(feature = "impl-platform-console")]
    {
        impl_platform_console::platform_console_write_a_buffer(text.as_bytes());
        return;
    }
    #[cfg(feature = "impl-dummy")]
    {
        use core::fmt::Write;
        let mut console = ConsoleHandle::default();
        let _ = console.write_str(text);
        return;
    }
    #[cfg(not(any(feature = "impl-platform-console", feature = "impl-dummy")))]
    {
        let _ = text;
    }
}

/// 将任意字节写入平台控制台（不要求 UTF-8）；供 `write` 系统调用等路径使用。
///
/// **契约**：无平台控制台后端时静默丢弃，避免在无串口构建中引入链接依赖；
/// 有后端时按字节原样下发。
/// OUTPUT_SYNC: 一次调用对应一次聚合层锁临界区，syscall 不应逐字节调用本函数。
#[inline]
pub fn write_raw_bytes(bytes : &[u8]) {
    #[cfg(feature = "impl-platform-console")]
    impl_platform_console::platform_console_write_raw_buffer(bytes);
    // 未链平台控制台实现时保持无操作，便于仅编译/单测场景。
    #[cfg(not(feature = "impl-platform-console"))]
    {
        let _ = bytes;
    }
}

/// 当前 feature 选中的默认控制台句柄类型。
///
/// 此类型用于 backend 实现和测试；普通 runtime 调用方不应直接实例化它。
#[cfg(feature = "impl-dummy")]
pub use impl_dummy::DummyConsoleHandle as ConsoleHandle;
#[cfg(feature = "impl-platform-console")]
pub use impl_platform_console::PlatformConsoleHandle as ConsoleHandle;

/// 使用当前 feature 选中的控制台后端格式化输出。
#[macro_export]
macro_rules! print {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::write_fmt(format_args!($fmt $(,$($arg)+)?));
    }
}
/// 使用当前 feature 选中的控制台后端格式化输出并自动追加换行。
#[macro_export]
macro_rules! println {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::write_fmt(format_args!(concat!($fmt, "\n") $(,$($arg)+)?));
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

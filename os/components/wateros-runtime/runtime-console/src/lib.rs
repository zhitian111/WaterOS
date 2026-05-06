#![no_std]
use core::fmt;
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
#[inline]
pub fn print<C : Console>(args : fmt::Arguments) {
    let mut c = C::default();
    c.write_fmt(args)
     .unwrap();
}
#[inline]
pub fn prints<C : Console>(str : &str) {
    let mut c = C::default();
    c.write_str(str)
     .unwrap();
}

#[cfg(feature = "impl-dummy")]
pub use impl_dummy::DummyConsoleHandle as ConsoleHandle;
#[cfg(any(feature = "impl-firmware-console", feature = "impl-firmware-opensbi"))]
pub use impl_firmware_opensbi::FirmwareConsoleHandle as ConsoleHandle;

#[macro_export]
macro_rules! print {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::print::<$crate::ConsoleHandle>(format_args!($fmt $(,$($arg)+)?));
    }
}
#[macro_export]
macro_rules! println {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::print::<$crate::ConsoleHandle>(format_args!(concat!($fmt, "\n") $(,$($arg)+)?));
    }
}
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

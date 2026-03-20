#![no_std]
#[inline]
#[allow(unused)]
pub fn firmware_write_a_byte(byte : u8) { firmware::console::console_write_a_byte(byte).unwrap(); }
#[inline]
#[allow(unused)]
pub fn firmware_write_a_buffer(bytes : &[u8]) {
    firmware::console::console_write_a_buffer(&bytes).unwrap();
}
use core::fmt::{self, Write};
// Console Trait 包括向控制台输出和从控制台读入的特性
pub trait Console: fmt::Write + Default {}
use firmware::console::FirmwareConsoleImpl;
#[derive(Default)]
pub struct FirmwareConsoleHandle;
impl Write for FirmwareConsoleHandle {
    #[inline]
    fn write_str(&mut self, s : &str) -> fmt::Result {
        firmware_write_a_buffer(s.as_bytes());
        Ok(())
    }
}
impl Console for FirmwareConsoleHandle {}
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
#[macro_export]
macro_rules! print {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::print::<$crate::FirmwareConsoleHandle>(format_args!($fmt $(,$($arg)+)?));
    }
}
#[macro_export]
macro_rules! println {
    ($fmt: literal $(, $($arg: tt)+)?) => {
        $crate::print::<$crate::FirmwareConsoleHandle>(format_args!(concat!($fmt, "\n") $(,$($arg)+)?));
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

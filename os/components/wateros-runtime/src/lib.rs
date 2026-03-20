#![no_std]
pub mod panic {
    pub use panic::panic_handler;
}
pub mod console {
    pub use console::*;
}

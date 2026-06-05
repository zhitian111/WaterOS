#![no_std]
//! 运行时串口聚合：经字符设备注册表访问 QEMU `virt` UART。

pub use character_api_v0::{CharacterDevice, SerialError, SerialPort, SerialResult};
pub use driver::uart::{
    init_default_virt_uart, read_byte_blocking, with_default_uart, QemuVirtUart16550,
    QEMU_VIRT_UART0_BASE,
};

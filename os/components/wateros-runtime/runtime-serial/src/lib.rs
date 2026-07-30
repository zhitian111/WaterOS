#![no_std]
//! 运行时串口聚合：经字符设备注册表访问 QEMU `virt` UART。
//!
//! PLATFORM_BOUNDARY: 这里是已注册字符设备的再导出，不是 early console；内核日志
//! 应继续经 `runtime-console → platform::console`，避免与 boot UART 路径竞争。

pub use character_api_v0::{CharacterDevice, SerialError, SerialPort, SerialResult};
pub use driver::uart::{
    init_default_virt_uart, read_byte_blocking, with_default_uart, QemuVirtUart16550,
    QEMU_VIRT_UART0_BASE,
};

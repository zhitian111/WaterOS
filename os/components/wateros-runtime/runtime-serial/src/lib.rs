#![no_std]
//! 运行时串口聚合：将 QEMU `virt` MMIO UART 与 [`SerialPort`] trait 暴露给伪 shell 等上层。
//!
//! 依赖 `wateros-driver` 在 `init_after_boot` 中完成 `init_default_virt_uart`；否则 `with_default_uart` 返回 `None`。

pub use character_api_v0::{SerialError, SerialPort, SerialResult};
pub use driver::uart::{init_default_virt_uart, with_default_uart, QemuVirtUart16550, QEMU_VIRT_UART0_BASE};

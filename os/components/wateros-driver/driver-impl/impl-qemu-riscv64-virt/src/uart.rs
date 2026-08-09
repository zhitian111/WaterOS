//! QEMU `virt` 上 NS16550 UART0（MMIO）：经 `impl-uart-16550` 的统一端口实现。
//!
//! 基址与 OpenSBI/QEMU 设备树常见布局一致：`0x1000_0000`。假定固件已完成线参数
//! 配置，本模块不做波特率除数编程，只负责平台侧基址/布局与字符设备注册。

use character::{
    register_uart_character_device as register_shared_uart, Ns16550Port, RegisterLayout,
};
use wateros_base_config::mm::QEMU_VIRT_MMIO_PHYS_START;
use wateros_driver_character_api_v0::CharacterDevice;

/// QEMU `virt` UART0 默认 MMIO 物理基址（与设备树常见 `serial@10000000` 一致）。
pub const QEMU_VIRT_UART0_BASE: usize = QEMU_VIRT_MMIO_PHYS_START;

/// QEMU `virt` UART0 寄存器布局：标准 16550 字节访问。
pub const QEMU_VIRT_UART_LAYOUT: RegisterLayout = RegisterLayout::Byte16550;

/// 以默认基址构造 QEMU `virt` UART 端口。
#[inline]
pub const fn qemu_virt_default_port() -> Ns16550Port {
    Ns16550Port::new(QEMU_VIRT_UART0_BASE, QEMU_VIRT_UART_LAYOUT)
}

/// 初始化并注册一个 UART 字符设备，返回注册表索引。
///
/// 当前平台（QEMU `virt`/DTB 扫描的 `ns16550a`/`ns8250` 节点）统一使用
/// [`RegisterLayout::Byte16550`]；后续真机若为 DW APB UART（如 JH7110）应改为
/// 直接调用共享 `register_uart_character_device(base, layout)`。
pub fn register_uart_character_device(base: usize) -> usize {
    register_shared_uart(base, RegisterLayout::Byte16550)
}

/// 取得首个已注册 UART 字符设备；未注册时返回 `None`。
pub fn with_default_uart<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut dyn CharacterDevice) -> R,
{
    character::with_character_device(0, f)
}

/// 与历史 API 兼容：字符设备注册后即为 ready。
pub fn init_default_virt_uart() {}

/// 阻塞读一字节（伪 shell 用）。
pub fn read_byte_blocking(dev: &mut dyn CharacterDevice) -> u8 {
    let mut b = [0u8];
    loop {
        match dev.read(&mut b) {
            Ok(1) => return b[0],
            Ok(0) | Err(_) => core::hint::spin_loop(),
            Ok(_) => core::hint::spin_loop(),
        }
    }
}

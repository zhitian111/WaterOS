//! QEMU LoongArch64 `virt` UART16550（MMIO）：经 `impl-uart-16550` 的统一端口实现。
//!
//! 基址与 QEMU `virt` 设备树中的 UART16550 `reg` 保持一致。早期控制台仍由
//! `wateros-platform` 的 board 层提供；本模块负责 driver 层可共享的串口对象与
//! 平台默认 UART 单例。

use core::sync::atomic::{AtomicBool, Ordering};

use character::{
    register_uart_character_device as register_shared_uart, Ns16550Port, RegisterLayout,
};
use spin::Mutex;

/// QEMU LoongArch64 `virt` UART16550 默认 MMIO 物理基址。
pub const QEMU_LOONGARCH64_UART16550_BASE: usize = 0x1FE0_01E0;

/// QEMU LoongArch64 `virt` UART 寄存器布局：标准 16550 字节访问。
pub const QEMU_LOONGARCH64_UART_LAYOUT: RegisterLayout = RegisterLayout::Byte16550;

/// 以默认基址构造 QEMU LoongArch64 `virt` UART 端口。
#[inline]
pub const fn qemu_virt_default_port() -> Ns16550Port {
    Ns16550Port::new(QEMU_LOONGARCH64_UART16550_BASE, QEMU_LOONGARCH64_UART_LAYOUT)
}

/// 初始化并注册 QEMU LoongArch64 `virt` 默认 UART 字符设备，返回注册表索引。
pub fn register_uart_character_device() -> usize {
    register_shared_uart(QEMU_LOONGARCH64_UART16550_BASE, RegisterLayout::Byte16550)
}

static UART_GLOBAL: Mutex<Option<Ns16550Port>> = Mutex::new(None);
static UART_INIT_DONE: AtomicBool = AtomicBool::new(false);

/// 将全局 UART 初始化为 QEMU LoongArch64 `virt` 默认 UART；幂等。
pub fn init_default_virt_uart() {
    // AcqRel 保证并发调用只初始化一次；后续调用直接返回且不会重复写 MMIO。
    if UART_INIT_DONE.swap(true, Ordering::AcqRel) {
        return;
    }
    let mut uart = qemu_virt_default_port();
    uart.init_minimal();
    *UART_GLOBAL.lock() = Some(uart);
}

/// 早期默认 UART 初始化别名；供启动路径在完整设备扫描前准备串口输出。
pub fn init_early_default_uart() {
    init_default_virt_uart();
}

/// 取得全局 UART 的可变访问；未初始化时返回 `None`。
///
/// 仅短暂持 `UART_GLOBAL` 复制句柄（`Copy`），I/O 在锁外完成，避免阻塞读占全局锁。
pub fn with_default_uart<F, R>(f: F) -> Option<R>
where
    F: FnOnce(&mut Ns16550Port) -> R,
{
    let mut uart = {
        let guard = UART_GLOBAL.lock();
        *guard.as_ref()?
    };
    Some(f(&mut uart))
}

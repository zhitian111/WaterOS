//! QEMU LoongArch64 `virt` UART16550（MMIO）：阻塞读 / 阻塞写，供运行期串口能力使用。
//!
//! 基址与 QEMU `virt` 设备树中的 UART16550 `reg` 保持一致。早期控制台仍由
//! `wateros-platform` 的 board 层提供；本模块负责 driver 层可共享的串口对象。

use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, Ordering};

use spin::Mutex;
use wateros_driver_character_api_v0::{SerialError, SerialPort, SerialResult};

/// QEMU LoongArch64 `virt` UART16550 默认 MMIO 物理基址。
pub const QEMU_LOONGARCH64_UART16550_BASE: usize = 0x1FE0_01E0;

const REG_THR: usize = 0;
const REG_RBR: usize = 0;
const REG_IER: usize = 1;
const REG_LSR: usize = 5;

const LSR_DATA_READY: u8 = 1;
const LSR_THRE: u8 = 1 << 5;

const SPIN_TX_MAX: usize = 1_000_000;

/// NS16550 风格 MMIO 串口；`base` 为物理/恒等映射内核地址。
#[derive(Debug, Clone, Copy)]
pub struct QemuLoongArch64Uart16550 {
    base: usize,
}

impl QemuLoongArch64Uart16550 {
    /// 使用给定 MMIO 基址构造句柄（不做探测）。
    #[inline]
    pub const fn new(base: usize) -> Self {
        Self { base }
    }

    /// QEMU LoongArch64 `virt` 默认 UART 实例。
    #[inline]
    pub const fn qemu_virt_default() -> Self {
        Self::new(QEMU_LOONGARCH64_UART16550_BASE)
    }

    #[inline]
    unsafe fn read_reg(&self, off: usize) -> u8 {
        unsafe { read_volatile((self.base + off) as *const u8) }
    }

    #[inline]
    unsafe fn write_reg(&self, off: usize, v: u8) {
        unsafe { write_volatile((self.base + off) as *mut u8, v) }
    }

    /// 关闭接收/发送中断使能，避免与轮询路径干扰。
    pub fn init_minimal(&mut self) {
        unsafe {
            self.write_reg(REG_IER, 0);
        }
    }

    fn write_byte_raw(&mut self, byte: u8) -> SerialResult<()> {
        let mut spins = 0usize;
        loop {
            let lsr = unsafe { self.read_reg(REG_LSR) };
            if (lsr & LSR_THRE) != 0 {
                unsafe {
                    self.write_reg(REG_THR, byte);
                }
                return Ok(());
            }
            spins = spins.saturating_add(1);
            if spins > SPIN_TX_MAX {
                return Err(SerialError::TransmitterStuck);
            }
            core::hint::spin_loop();
        }
    }
}

impl SerialPort for QemuLoongArch64Uart16550 {
    fn write_byte(&mut self, byte: u8) -> SerialResult<()> {
        if byte == b'\n' {
            self.write_byte_raw(b'\r')?;
        }
        self.write_byte_raw(byte)
    }

    fn read_byte_blocking(&mut self) -> u8 {
        loop {
            if let Some(b) = self.try_read_byte() {
                return b;
            }
            core::hint::spin_loop();
        }
    }

    fn try_read_byte(&mut self) -> Option<u8> {
        let lsr = unsafe { self.read_reg(REG_LSR) };
        if (lsr & LSR_DATA_READY) != 0 {
            let b = unsafe { self.read_reg(REG_RBR) };
            return Some(b);
        }
        None
    }
}

static UART_GLOBAL: Mutex<Option<QemuLoongArch64Uart16550>> = Mutex::new(None);
static UART_INIT_DONE: AtomicBool = AtomicBool::new(false);

/// 将全局 UART 初始化为 QEMU LoongArch64 `virt` 默认 UART；幂等。
pub fn init_default_virt_uart() {
    if UART_INIT_DONE.swap(true, Ordering::AcqRel) {
        return;
    }
    let mut uart = QemuLoongArch64Uart16550::qemu_virt_default();
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
    F: FnOnce(&mut QemuLoongArch64Uart16550) -> R,
{
    let mut uart = {
        let guard = UART_GLOBAL.lock();
        *guard.as_ref()?
    };
    Some(f(&mut uart))
}

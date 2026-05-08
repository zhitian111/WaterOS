//! QEMU `virt` 上 NS16550 UART0（MMIO）：阻塞读 / 阻塞写，供伪 shell 与 bring-up 串口交互。
//!
//! 基址与 OpenSBI/QEMU 设备树常见布局一致：`0x1000_0000`。假定固件已完成线参数配置，本模块不做波特率除数编程。

use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, Ordering};

use spin::Mutex;
use wateros_driver_character_api_v0::{SerialError, SerialPort, SerialResult};

use wateros_base_config::mm::QEMU_VIRT_MMIO_PHYS_START;

/// QEMU `virt` UART0 默认 MMIO 物理基址（与设备树常见 `serial@10000000` 一致）。
pub const QEMU_VIRT_UART0_BASE : usize = QEMU_VIRT_MMIO_PHYS_START;

const REG_THR : usize = 0;
const REG_IER : usize = 1;
const REG_LSR : usize = 5;

const LSR_DATA_READY : u8 = 1;
const LSR_THRE : u8 = 1 << 5;

const SPIN_TX_MAX : usize = 1_000_000;

/// NS16550 风格 MMIO 串口；`base` 为物理/恒等映射内核地址。
#[derive(Debug, Clone, Copy)]
pub struct QemuVirtUart16550 {
    base : usize,
}

impl QemuVirtUart16550 {
    /// 使用给定 MMIO 基址构造句柄（不做探测）。
    #[inline]
    pub const fn new(base : usize) -> Self {
        Self { base }
    }

    /// QEMU `virt` UART0 默认实例。
    #[inline]
    pub const fn qemu_virt_default() -> Self {
        Self::new(QEMU_VIRT_UART0_BASE)
    }

    #[inline]
    unsafe fn read_reg(&self, off : usize) -> u8 {
        unsafe { read_volatile((self.base + off) as *const u8) }
    }

    #[inline]
    unsafe fn write_reg(&self, off : usize, v : u8) {
        unsafe { write_volatile((self.base + off) as *mut u8, v) }
    }

    /// 关闭接收/发送中断使能，避免与轮询路径干扰。
    pub fn init_minimal(&mut self) {
        unsafe {
            self.write_reg(REG_IER, 0);
        }
    }

    fn write_byte_raw(&mut self, byte : u8) -> SerialResult<()> {
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

impl SerialPort for QemuVirtUart16550 {
    fn write_byte(&mut self, byte : u8) -> SerialResult<()> {
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
            let b = unsafe { self.read_reg(REG_THR) };
            return Some(b);
        }
        None
    }
}

static UART_GLOBAL : Mutex<Option<QemuVirtUart16550>> = Mutex::new(None);
static UART_INIT_DONE : AtomicBool = AtomicBool::new(false);

/// 将全局 UART 初始化为 QEMU `virt` UART0；幂等。
pub fn init_default_virt_uart() {
    if UART_INIT_DONE.swap(true, Ordering::AcqRel) {
        return;
    }
    let mut u = QemuVirtUart16550::qemu_virt_default();
    u.init_minimal();
    *UART_GLOBAL.lock() = Some(u);
}

/// 取得全局 UART 的可变访问；未 [`init_default_virt_uart`] 时返回 `None`。
pub fn with_default_uart<F, R>(f : F) -> Option<R>
where F : FnOnce(&mut QemuVirtUart16550) -> R {
    let mut g = UART_GLOBAL.lock();
    let u = g.as_mut()?;
    Some(f(u))
}

//! QEMU `virt` 上 NS16550 UART0（MMIO）：阻塞读 / 阻塞写，供伪 shell 与 bring-up 串口交互。
//!
//! 基址与 OpenSBI/QEMU 设备树常见布局一致：`0x1000_0000`。假定固件已完成线参数配置，本模块不做波特率除数编程。

use core::ptr::{read_volatile, write_volatile};

use wateros_driver_character_api_v0::{CharacterDevice, SerialError, SerialPort, SerialResult};

use wateros_base_config::mm::QEMU_VIRT_MMIO_PHYS_START;

/// QEMU `virt` UART0 默认 MMIO 物理基址（与设备树常见 `serial@10000000` 一致）。
pub const QEMU_VIRT_UART0_BASE: usize = QEMU_VIRT_MMIO_PHYS_START;

const REG_THR: usize = 0;
const REG_IER: usize = 1;
const REG_LSR: usize = 5;

const LSR_DATA_READY: u8 = 1;
const LSR_THRE: u8 = 1 << 5;
const SPIN_TX_MAX: usize = 1_000_000;
/// NS16550 风格 MMIO 串口；`base` 为物理/恒等映射内核地址。
#[derive(Debug, Clone, Copy)]
pub struct QemuVirtUart16550 {
    pub base: usize,
}

impl QemuVirtUart16550 {
    /// 使用给定 MMIO 基址构造句柄（不做探测）。
    #[inline]
    pub const fn new(base: usize) -> Self {
        Self { base }
    }

    /// QEMU `virt` UART0 默认实例。
    #[inline]
    pub const fn qemu_virt_default() -> Self {
        Self::new(QEMU_VIRT_UART0_BASE)
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

}

impl SerialPort for QemuVirtUart16550 {
    fn write_byte(&mut self, byte: u8) -> SerialResult<()> {
        for _ in 0..SPIN_TX_MAX {
            if unsafe { self.read_reg(REG_LSR) } & LSR_THRE != 0 {
                unsafe { self.write_reg(REG_THR, byte) };
                return Ok(());
            }
            core::hint::spin_loop();
        }
        Err(SerialError::TransmitterStuck)
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

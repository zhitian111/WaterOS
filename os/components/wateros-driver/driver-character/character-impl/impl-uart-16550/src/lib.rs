//! 可配置的 NS16550 家族 MMIO 串口驱动（QEMU RV/LA、龙芯 2K1000、JH7110 共用）。
//!
//! 统一处理 16550 字节布局与 DesignWare APB 32 位布局（`reg-shift=2`）。平台层只负责
//! 传入基址与寄存器布局并注册为字符设备；本 crate 不感知具体板级地址，也不做波特率
//! 除数编程（假定固件已完成线参数配置）。

#![no_std]
extern crate alloc;

use alloc::{boxed::Box, sync::Arc};
use core::ptr::{read_volatile, write_volatile};

use character_api_v0::{
    register_character_device, CharacterDevice, SerialError, SerialPort,
    SerialPortCharacterDevice, SerialResult, SharedCharacterDevice,
};
use spin::Mutex;

/// 16550 家族寄存器布局（决定寄存器偏移与 MMIO 访问宽度）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterLayout {
    /// 标准 16550A 字节布局：THR=0、IER=1、LSR=5，8 位访问。
    /// QEMU RISC-V/LoongArch `virt` 与龙芯 2K1000 使用。
    Byte16550,
    /// DesignWare APB UART（`reg-shift=2`、`reg-io-width=4`）：THR=0、IER=4、
    /// LSR=0x14，32 位访问。JH7110/VisionFive 2 使用；真机验证前仅保证编译正确。
    DwApb32,
}

const REG_THR: usize = 0;
const REG_IER: usize = 1;
const REG_LSR: usize = 5;
const LSR_DATA_READY: u8 = 1;
const LSR_THRE: u8 = 1 << 5;
const SPIN_TX_MAX: usize = 1_000_000;

impl RegisterLayout {
    /// 16550 寄存器序号到 MMIO 字节偏移的换算。
    #[inline]
    const fn reg_offset(self, index: usize) -> usize {
        match self {
            RegisterLayout::Byte16550 => index,
            RegisterLayout::DwApb32 => index << 2,
        }
    }

    #[inline]
    unsafe fn read_reg(self, base: usize, index: usize) -> u8 {
        let offset = self.reg_offset(index);
        match self {
            RegisterLayout::Byte16550 => {
                unsafe { read_volatile((base + offset) as *const u8) }
            }
            RegisterLayout::DwApb32 => unsafe { read_volatile((base + offset) as *const u32) as u8 },
        }
    }

    #[inline]
    unsafe fn write_reg(self, base: usize, index: usize, value: u8) {
        let offset = self.reg_offset(index);
        match self {
            RegisterLayout::Byte16550 => {
                unsafe { write_volatile((base + offset) as *mut u8, value) }
            }
            RegisterLayout::DwApb32 => {
                unsafe { write_volatile((base + offset) as *mut u32, value as u32) }
            }
        }
    }
}

/// NS16550 风格 MMIO 串口；`base` 为物理/恒等映射内核地址。
#[derive(Debug, Clone, Copy)]
pub struct Ns16550Port {
    base: usize,
    layout: RegisterLayout,
}

impl Ns16550Port {
    /// 使用给定 MMIO 基址与寄存器布局构造句柄（不做探测）。
    #[inline]
    pub const fn new(base: usize, layout: RegisterLayout) -> Self {
        Self { base, layout }
    }

    /// MMIO 物理/恒等映射基址。
    #[inline]
    pub const fn base(&self) -> usize {
        self.base
    }

    /// 寄存器布局。
    #[inline]
    pub const fn layout(&self) -> RegisterLayout {
        self.layout
    }

    /// 关闭接收/发送中断使能，避免与轮询路径干扰。
    pub fn init_minimal(&mut self) {
        unsafe { self.layout.write_reg(self.base, REG_IER, 0) }
    }
}

impl SerialPort for Ns16550Port {
    fn write_byte(&mut self, byte: u8) -> SerialResult<()> {
        // 设置上限避免硬件永不置位 THRE 时无限自旋；超时转为可传播错误。
        for _ in 0..SPIN_TX_MAX {
            let lsr = unsafe { self.layout.read_reg(self.base, REG_LSR) };
            if lsr & LSR_THRE != 0 {
                unsafe { self.layout.write_reg(self.base, REG_THR, byte) };
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
        let lsr = unsafe { self.layout.read_reg(self.base, REG_LSR) };
        if lsr & LSR_DATA_READY != 0 {
            let b = unsafe { self.layout.read_reg(self.base, REG_THR) };
            return Some(b);
        }
        None
    }
}

/// 初始化并注册一个 UART 字符设备，返回注册表索引。
pub fn register_uart_character_device(base: usize, layout: RegisterLayout) -> usize {
    let mut port = Ns16550Port::new(base, layout);
    port.init_minimal();
    let shared: SharedCharacterDevice = Arc::new(Mutex::new(
        Box::new(SerialPortCharacterDevice::new(port)) as Box<dyn CharacterDevice>,
    ));
    register_character_device(shared)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte16550_offsets() {
        let layout = RegisterLayout::Byte16550;
        assert_eq!(layout.reg_offset(REG_THR), 0);
        assert_eq!(layout.reg_offset(REG_IER), 1);
        assert_eq!(layout.reg_offset(REG_LSR), 5);
    }

    #[test]
    fn dwapb32_offsets() {
        let layout = RegisterLayout::DwApb32;
        assert_eq!(layout.reg_offset(REG_THR), 0);
        assert_eq!(layout.reg_offset(REG_IER), 4);
        assert_eq!(layout.reg_offset(REG_LSR), 0x14);
    }
}

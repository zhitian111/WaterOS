//! LoongArch QEMU virt EIOINTC → PCH-PIC 外部中断路径。
//!
//! 拓扑：EIOINTC（IOCSR 地址空间）汇总系统中断并路由到 CPU 硬件中断输入 3；
//! PCH-PIC（MMIO `0x1000_0000`）接收 PCI INTx（向量 16..19）。LoongArch 内核
//! 通过 DMW0 直接映射窗口访问 MMIO，无需额外页表映射。

use core::arch::asm;
use core::ptr::write_volatile;

const EIO_ENABLE : usize = 0x1600;
const EIO_ISR : usize = 0x1800;
const EIO_ROUTE : usize = 0x1C00;
const PCH_BASE : usize = 0x1000_0000;
const PCH_MASK : usize = 0x20;
const PCH_CLR : usize = 0x80;
const PCH_PCI_FIRST : u32 = 16;
const PCH_PCI_LAST : u32 = 19;

/// EIOINTC 操作错误（无字段的单元错误）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExternalIrqError;

#[inline]
fn iocsr_read32(address : usize) -> u32 {
    let value : u32;
    unsafe {
        asm!("iocsrrd.w {value}, {address}",
             value = out(reg) value,
             address = in(reg) address,
             options(nostack));
    }
    value
}

#[inline]
fn iocsr_write32(value : u32, address : usize) {
    unsafe {
        asm!("iocsrwr.w {value}, {address}",
             value = in(reg) value,
             address = in(reg) address,
             options(nostack));
    }
}

// SAFETY: `address` 必须落在经 DMW 直接映射可访问的 PCH-PIC MMIO 窗口内。
#[inline]
unsafe fn mmio_write32(address : usize, value : u32) {
    unsafe {
        write_volatile(address as *mut u32, value);
    }
}

/// 初始化当前 CPU：使能全部 EIOINTC 向量、把 PCI INTx 16..19 路由到该 CPU、
/// 打开 PCH-PIC（清 mask / 清 pending）。
pub fn init_current_cpu(cpu : usize) -> Result<(), ExternalIrqError> {
    for word in 0..4 {
        iocsr_write32(u32::MAX, EIO_ENABLE + word * 4);
    }
    for vector in PCH_PCI_FIRST..=PCH_PCI_LAST {
        let offset = EIO_ROUTE + (vector as usize & !3);
        let shift = (vector & 3) * 8;
        let old = iocsr_read32(offset);
        iocsr_write32((old & !(0xFF << shift)) | ((cpu as u32 & 0xFF) << shift),
                      offset);
    }
    // SAFETY: PCH-PIC MMIO 经 LoongArch DMW 直接映射窗口可访问。
    unsafe {
        mmio_write32(PCH_BASE + PCH_MASK, u32::MAX);
        mmio_write32(PCH_BASE + PCH_CLR, u32::MAX);
        mmio_write32(PCH_BASE + PCH_MASK, 0);
    }
    Ok(())
}

/// 使能/关断 EIOINTC 向量（按向量号定位 enable 寄存器字）。
pub fn set_enabled(irq : u32, _cpu : usize, enabled : bool) -> Result<(), ExternalIrqError> {
    let word = (irq / 32) as usize;
    let bit = 1u32 << (irq % 32);
    if word >= 4 {
        return Err(ExternalIrqError);
    }
    let old = iocsr_read32(EIO_ENABLE + word * 4);
    iocsr_write32(if enabled { old | bit } else { old & !bit },
                  EIO_ENABLE + word * 4);
    Ok(())
}

/// claim 当前 CPU 一个 pending EIOINTC 向量；无 pending 返回 `None`。
pub fn claim(_cpu : usize) -> Option<u32> {
    for word in 0..4 {
        let pending = iocsr_read32(EIO_ISR + word * 4);
        if pending != 0 {
            return Some((word * 32 + pending.trailing_zeros() as usize) as u32);
        }
    }
    None
}

/// complete（EOI）：仅 PCI INTx 向量需要清 PCH-PIC pending。
pub fn complete(_cpu : usize, irq : u32) {
    if !(PCH_PCI_FIRST..=PCH_PCI_LAST).contains(&irq) {
        return;
    }
    // SAFETY: 同上。
    unsafe {
        mmio_write32(PCH_BASE + PCH_CLR, 1u32 << irq);
    }
}

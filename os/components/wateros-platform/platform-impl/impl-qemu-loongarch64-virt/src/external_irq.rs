//! LoongArch QEMU virt EIOINTC → PCH-PIC external IRQ path.

use core::arch::asm;
use core::ptr::write_volatile;

const EIO_ENABLE: usize = 0x1600;
const EIO_ISR: usize = 0x1800;
const EIO_ROUTE: usize = 0x1c00;
const PCH_BASE: usize = 0x1000_0000;
const PCH_MASK: usize = 0x20;
const PCH_CLR: usize = 0x80;
const PCH_PCI_FIRST: u32 = 16;
const PCH_PCI_LAST: u32 = 19;

#[inline]
fn iocsr_read32(address: usize) -> u32 {
    let value: u32;
    unsafe { asm!("iocsrrd.w {value}, {address}", value = out(reg) value, address = in(reg) address, options(nostack)); }
    value
}

#[inline]
fn iocsr_write32(value: u32, address: usize) {
    unsafe { asm!("iocsrwr.w {value}, {address}", value = in(reg) value, address = in(reg) address, options(nostack)); }
}

#[inline]
unsafe fn mmio_write32(address: usize, value: u32) { unsafe { write_volatile(address as *mut u32, value); } }

pub fn init_for_current_cpu(cpu: usize) -> Result<(), ()> {
    // Enable all EIOINTC vectors and route the four PCI INTx vectors to the
    // current CPU. QEMU's virt machine has one EIO node and uses vector 16..19.
    for word in 0..4 { iocsr_write32(u32::MAX, EIO_ENABLE + word * 4); }
    for vector in PCH_PCI_FIRST..=PCH_PCI_LAST {
        let offset = EIO_ROUTE + (vector as usize & !3);
        let shift = (vector & 3) * 8;
        let old = iocsr_read32(offset);
        iocsr_write32((old & !(0xff << shift)) | ((cpu as u32 & 0xff) << shift), offset);
    }
    unsafe {
        mmio_write32(PCH_BASE + PCH_MASK, u32::MAX);
        mmio_write32(PCH_BASE + PCH_CLR, u32::MAX);
        mmio_write32(PCH_BASE + PCH_MASK, 0);
    }
    Ok(())
}

pub fn claim(_cpu: usize) -> Option<u32> {
    for word in 0..4 {
        let pending = iocsr_read32(EIO_ISR + word * 4);
        if pending != 0 {
            return Some((word * 32 + pending.trailing_zeros() as usize) as u32);
        }
    }
    None
}

pub fn complete(_cpu: usize, irq: u32) {
    if !(PCH_PCI_FIRST..=PCH_PCI_LAST).contains(&irq) { return; }
    let bit = 1u32 << irq;
    unsafe { mmio_write32(PCH_BASE + PCH_CLR, bit); }
}

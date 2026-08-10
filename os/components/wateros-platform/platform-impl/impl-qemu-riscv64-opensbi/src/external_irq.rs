//! QEMU virt PLIC supervisor-context access.

use core::ptr::{read_volatile, write_volatile};


const PLIC_BASE: usize = 0x0c00_0000;
// QEMU virt PLIC layout: priority 0x0, pending 0x1000, per-context enables
// at 0x2000, and per-context threshold/claim at 0x200000.
const PLIC_ENABLE_BASE: usize = 0x0000_2000;
const PLIC_CONTEXT_BASE: usize = 0x0020_0000;
const PLIC_CONTEXT_STRIDE: usize = 0x1000;
const PLIC_ENABLE_STRIDE: usize = 0x80;
const PLIC_MAX_IRQ: u32 = 95;

#[inline]
unsafe fn write32(addr: usize, value: u32) {
    unsafe { write_volatile(addr as *mut u32, value); }
}

#[inline]
unsafe fn read32(addr: usize) -> u32 {
    unsafe { read_volatile(addr as *const u32) }
}

#[inline]
fn context_for_hart(hart: usize) -> usize { hart * 2 + 1 }

/// 初始化当前 hart 的 PLIC supervisor context，并开启全部已知 QEMU virt IRQ。
pub fn init_for_current_hart(hart: usize) -> Result<(), ()> {
    let context = context_for_hart(hart);
    let enable = PLIC_BASE + PLIC_ENABLE_BASE + context * PLIC_ENABLE_STRIDE;
    let threshold = PLIC_BASE + PLIC_CONTEXT_BASE + context * PLIC_CONTEXT_STRIDE;
    unsafe {
        write32(threshold, 0);
        for word in 0..3 {
            write32(enable + word * 4, u32::MAX);
        }
        // PLIC source 0 is reserved; priority 1 is sufficient for QEMU virt.
        for irq in 1..=PLIC_MAX_IRQ {
            write32(PLIC_BASE + irq as usize * 4, 1);
        }
    }
    Ok(())
}

/// 从当前 hart 的 supervisor context claim 一个设备 IRQ。
pub fn claim(hart: usize) -> Option<u32> {
    let context = context_for_hart(hart);
    let claim = PLIC_BASE + PLIC_CONTEXT_BASE + context * PLIC_CONTEXT_STRIDE + 4;
    let irq = unsafe { read32(claim) };
    (irq != 0).then_some(irq)
}

/// 完成一个已 claim 的 IRQ。
pub fn complete(hart: usize, irq: u32) {
    if irq == 0 || irq > PLIC_MAX_IRQ { return; }
    let context = context_for_hart(hart);
    let claim = PLIC_BASE + PLIC_CONTEXT_BASE + context * PLIC_CONTEXT_STRIDE + 4;
    unsafe { write32(claim, irq); }
}

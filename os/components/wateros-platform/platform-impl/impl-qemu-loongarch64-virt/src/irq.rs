//! QEMU virt PCH PIC -> EXTIOI -> BSP HWI0 level-triggered IRQ routing.
//!
//! IRQ numbers are PCH input pins, not firmware GSI numbers (which add 64).
//! This board backend deliberately uses legacy one-hot EXTIOI CPU routing.

use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, Ordering};

const PIC : usize = 0x1000_0000;
static READY : AtomicBool = AtomicBool::new(false);

fn read(offset : usize) -> u32 {
    // SAFETY: PCH PIC is in the platform's mapped MMIO aperture.
    unsafe { read_volatile((PIC + offset) as *const u32) }
}
fn write(offset : usize, value : u32) {
    // SAFETY: offsets select the QEMU PCH PIC's 32-bit registers.
    unsafe { write_volatile((PIC + offset) as *mut u32, value) }
}
fn io_read(address : usize) -> u32 {
    let value : u32;
    // SAFETY: caller runs in PLV0, with a valid EXTIOI IOCSR address.
    unsafe {
        asm!("iocsrrd.w {value}, {address}", value = out(reg) value,
                  address = in(reg) address, options(nostack));
    }
    value
}
fn io_write(address : usize, value : u32) {
    // SAFETY: caller runs in PLV0, with a valid EXTIOI IOCSR address.
    unsafe {
        asm!("iocsrwr.w {value}, {address}", value = in(reg) value,
                  address = in(reg) address, options(nostack));
    }
}

/// Initialize once on the BSP, with global interrupts masked.
pub fn init(cpu_id : usize) -> Result<(), &'static str> {
    if cpu_id >= 4 {
        return Err("EXTIOI legacy routing supports CPUs 0..3");
    }
    if READY.load(Ordering::Acquire) {
        return Err("EXTIOI already initialized");
    }
    for word in 0..2 {
        write(0x20 + word * 4, u32::MAX); // mask all PCH inputs
        write(0x60 + word * 4, 0); // level-triggered
        write(0x3E0 + word * 4, 0); // active high
    }
    for word in 0..8 {
        io_write(0x1600 + word * 4, 0);
    }
    // Each byte routes 32 sources to EXTIOI output 0, connected to CPU HWI0.
    io_write(0x14C0, 0x0101_0101);
    io_write(0x14C4, 0x0101_0101);
    let route = (1u32 << cpu_id) * 0x0101_0101;
    for word in 0..8 {
        io_write(0x1C00 + word * 4, route);
    }
    for irq in 0..32 {
        // SAFETY: HTMSI vector table is byte-addressable and has 64 entries (32 wired on QEMU virt).
        unsafe {
            write_volatile((PIC + 0x200 + irq) as *mut u8,
                           irq as u8);
        }
    }
    READY.store(true, Ordering::Release);
    Ok(())
}

/// Unmask one PCH pin; caller must be BSP with local interrupts masked.
pub fn enable(irq : u32) -> bool {
    if irq >= 32 || !READY.load(Ordering::Acquire) {
        return false;
    }
    let word = irq as usize / 32 * 4;
    let bit = 1u32 << (irq % 32);
    io_write(0x1600 + word,
             io_read(0x1600 + word) | bit);
    write(0x20 + word, read(0x20 + word) & !bit);
    true
}

/// Mask one PCH pin; caller must be BSP with local interrupts masked.
pub fn disable(irq : u32) {
    if irq >= 32 || !READY.load(Ordering::Acquire) {
        return;
    }
    let word = irq as usize / 32 * 4;
    let bit = 1u32 << (irq % 32);
    write(0x20 + word, read(0x20 + word) | bit);
    io_write(0x1600 + word,
             io_read(0x1600 + word) & !bit);
}

/// Claim a pending source by clearing its EXTIOI latch before device service.
/// Clearing after service could discard a newly arrived device notification.
pub fn claim() -> Option<u32> {
    if !READY.load(Ordering::Acquire) {
        return None;
    }
    for word in 0..2 {
        let pending = io_read(0x1800 + word * 4);
        if pending != 0 {
            let bit = pending.trailing_zeros();
            io_write(0x1800 + word * 4, 1 << bit);
            return Some(word as u32 * 32 + bit);
        }
    }
    None
}

/// Level-triggered PCH sources deassert when the driver acknowledges the device.
pub fn complete(_irq : u32) {
    // Order device MMIO acknowledgement before exception return.
    unsafe {
        asm!("dbar 0", options(nostack));
    }
}

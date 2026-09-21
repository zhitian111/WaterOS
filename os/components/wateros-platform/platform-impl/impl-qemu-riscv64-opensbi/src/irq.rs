//! QEMU virt PLIC. Device sources are routed to the boot hart S-mode context.

use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicUsize, Ordering};

const BASE : usize = 0x0C00_0000;
const MAX_IRQ : u32 = 95;
static CONTEXT : AtomicUsize = AtomicUsize::new(usize::MAX);

fn read(offset : usize) -> u32 {
    // SAFETY: the caller initialized the board with the PLIC MMIO window mapped.
    unsafe { read_volatile((BASE + offset) as *const u32) }
}
fn write(offset : usize, value : u32) {
    // SAFETY: register offsets are bounded by the QEMU virt PLIC register map.
    unsafe { write_volatile((BASE + offset) as *mut u32, value) }
}
fn context() -> usize { CONTEXT.load(Ordering::Acquire) }

/// Initialize once, on the BSP with local interrupts masked and PLIC MMIO mapped.
pub fn init(cpu_id : usize) -> Result<(), &'static str> {
    // QEMU virt supplies machine and supervisor contexts for each hart.
    if cpu_id >= 512 {
        return Err("PLIC hart out of range");
    }
    let context = cpu_id * 2 + 1;
    if CONTEXT.compare_exchange(usize::MAX,
                                context,
                                Ordering::AcqRel,
                                Ordering::Acquire)
              .is_err()
    {
        return Err("PLIC already initialized");
    }
    for word in 0..3 {
        write(0x2000 + context * 0x80 + word * 4, 0);
    }
    write(0x20_0000 + context * 0x1000, 0);
    Ok(())
}

/// Unmask a valid source on the BSP; local interrupts must be masked.
pub fn enable(irq : u32) -> bool {
    if irq == 0 || irq > MAX_IRQ || context() == usize::MAX {
        return false;
    }
    write(irq as usize * 4, 1);
    let offset = 0x2000 + context() * 0x80 + irq as usize / 32 * 4;
    write(offset, read(offset) | (1 << (irq % 32)));
    true
}

/// Mask a source on the BSP; local interrupts must be masked.
pub fn disable(irq : u32) {
    if irq == 0 || irq > MAX_IRQ || context() == usize::MAX {
        return;
    }
    let offset = 0x2000 + context() * 0x80 + irq as usize / 32 * 4;
    write(offset,
          read(offset) & !(1 << (irq % 32)));
}

/// Claim one pending source, on the BSP hard-interrupt path.
pub fn claim() -> Option<u32> {
    if context() == usize::MAX {
        return None;
    }
    let irq = read(0x20_0004 + context() * 0x1000);
    (irq != 0).then_some(irq)
}

/// Complete a claimed source after acknowledging the device.
pub fn complete(irq : u32) {
    if context() != usize::MAX && irq != 0 {
        // Order device acknowledgement before controller completion.
        unsafe {
            core::arch::asm!("fence iorw, iorw", options(nostack));
        }
        write(0x20_0004 + context() * 0x1000, irq);
    }
}

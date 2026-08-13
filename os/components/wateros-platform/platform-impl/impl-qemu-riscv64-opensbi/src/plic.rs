//! QEMU RISC-V `virt` PLIC S-mode support.

use core::ptr;
use core::sync::atomic::{AtomicUsize, Ordering};

const PLIC_BASE: usize = 0x0c00_0000;
const ENABLE_BASE: usize = 0x002000;
const CONTEXT_BASE: usize = 0x0020_0000;
const ENABLE_STRIDE: usize = 0x80;
const CONTEXT_STRIDE: usize = 0x1000;
const MAX_IRQS: usize = 64;

type IrqHandler = fn(u32);

static IRQ_HANDLERS: [AtomicUsize; MAX_IRQS] =
    [const { AtomicUsize::new(0) }; MAX_IRQS];

const fn scontext(hart: usize) -> usize {
    hart * 2 + 1
}

fn current_hart_id() -> usize {
    let hart: usize;
    // SAFETY: _start.S and trap entry install `tp` before Rust code runs.
    unsafe {
        core::arch::asm!("mv {}, tp", out(reg) hart, options(nomem, nostack));
    }
    hart
}

pub fn register_handler(irq: u32, handler: IrqHandler) {
    if let Some(slot) = IRQ_HANDLERS.get(irq as usize) {
        slot.store(handler as usize, Ordering::Release);
    }
}

pub fn enable_current_context(irq: u32) {
    let hart = current_hart_id();
    let word = irq / 32;
    let bit = 1u32 << (irq % 32);
    let enable = PLIC_BASE + ENABLE_BASE + scontext(hart) * ENABLE_STRIDE + word as usize * 4;
    let threshold = PLIC_BASE + CONTEXT_BASE + scontext(hart) * CONTEXT_STRIDE;
    // SAFETY: QEMU PLIC MMIO is identity mapped by the kernel page table.
    unsafe {
        ptr::write_volatile((PLIC_BASE + irq as usize * 4) as *mut u32, 1);
        let old = ptr::read_volatile(enable as *const u32);
        ptr::write_volatile(enable as *mut u32, old | bit);
        ptr::write_volatile(threshold as *mut u32, 0);
    }
}

pub fn claim() -> u32 {
    let addr = PLIC_BASE + CONTEXT_BASE + scontext(current_hart_id()) * CONTEXT_STRIDE + 4;
    // SAFETY: QEMU PLIC MMIO is identity mapped by the kernel page table.
    unsafe { ptr::read_volatile(addr as *const u32) }
}

pub fn complete(irq: u32) {
    let addr = PLIC_BASE + CONTEXT_BASE + scontext(current_hart_id()) * CONTEXT_STRIDE + 4;
    // SAFETY: QEMU PLIC MMIO is identity mapped by the kernel page table.
    unsafe {
        ptr::write_volatile(addr as *mut u32, irq);
    }
}

pub fn handle_external_interrupt() {
    loop {
        let irq = claim();
        if irq == 0 {
            break;
        }
        if let Some(slot) = IRQ_HANDLERS.get(irq as usize) {
            let handler = slot.load(Ordering::Acquire);
            if handler != 0 {
                // SAFETY: registration stores a valid function pointer.
                let f: IrqHandler = unsafe { core::mem::transmute(handler) };
                f(irq);
            }
        }
        complete(irq);
    }
}

//! Loongson 2K1000 LIOINTC ownership for the BSP-only external IRQ path.

#[cfg(target_arch = "loongarch64")]
use core::sync::atomic::{AtomicBool, Ordering};

const MAX_IRQS : u32 = 64;
const LIOINTC_MAIN_BASE : usize = 0x1FE0_1400;
const LIOINTC_ISR0_BASE : usize = 0x1FE0_1540;

#[cfg(target_arch = "loongarch64")]
static CONTROLLER : irq_loongarch::liointc::Liointc =
    unsafe { irq_loongarch::liointc::Liointc::new(LIOINTC_MAIN_BASE, LIOINTC_ISR0_BASE) };
#[cfg(target_arch = "loongarch64")]
static READY : AtomicBool = AtomicBool::new(false);

pub fn init(_cpu_id : usize) -> Result<(), &'static str> {
    #[cfg(target_arch = "loongarch64")]
    {
        if READY.load(Ordering::Acquire) {
            return Err("LIOINTC already initialized");
        }
        CONTROLLER.init();
        READY.store(true, Ordering::Release);
        Ok(())
    }
    #[cfg(not(target_arch = "loongarch64"))]
    Err("LIOINTC is only available on LoongArch64")
}

pub fn enable(irq : u32) -> bool {
    #[cfg(target_arch = "loongarch64")]
    {
        if irq >= MAX_IRQS || !READY.load(Ordering::Acquire) {
            return false;
        }
        CONTROLLER.enable_irq(irq as usize);
        true
    }
    #[cfg(not(target_arch = "loongarch64"))]
    {
        let _ = irq;
        false
    }
}

pub fn disable(irq : u32) {
    #[cfg(target_arch = "loongarch64")]
    if irq < MAX_IRQS && READY.load(Ordering::Acquire) {
        CONTROLLER.disable_irq(irq as usize);
    }
    #[cfg(not(target_arch = "loongarch64"))]
    let _ = irq;
}

pub fn claim() -> Option<u32> {
    #[cfg(target_arch = "loongarch64")]
    {
        if !READY.load(Ordering::Acquire) {
            return None;
        }
        CONTROLLER.claim_irq()
                  .map(|irq| irq as u32)
    }
    #[cfg(not(target_arch = "loongarch64"))]
    None
}

pub fn complete(irq : u32) {
    #[cfg(target_arch = "loongarch64")]
    if irq < MAX_IRQS && READY.load(Ordering::Acquire) {
        // Order device acknowledgement before controller completion.
        unsafe {
            core::arch::asm!("dbar 0", options(nostack));
        }
        CONTROLLER.complete_irq(irq as usize);
    }
    #[cfg(not(target_arch = "loongarch64"))]
    let _ = irq;
}

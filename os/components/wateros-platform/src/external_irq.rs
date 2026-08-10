//! QEMU RISC-V PLIC operations used by the generic IRQ layer.

#[derive(Clone, Copy, Debug)]
pub struct ExternalIrqError;

#[cfg(feature = "impl-qemu-riscv64-opensbi")]
mod active {
    use super::ExternalIrqError;

    const PLIC_BASE : usize = 0x0C00_0000;
    const PLIC_MAX_IRQ : u32 = 127;
    const PRIORITY_BASE : usize = PLIC_BASE;
    const PENDING_BASE : usize = PLIC_BASE + 0x1000;
    const ENABLE_BASE : usize = PLIC_BASE + 0x2000;
    const ENABLE_STRIDE : usize = 0x80;
    const CONTEXT_BASE : usize = PLIC_BASE + 0x20_0000;
    const CONTEXT_STRIDE : usize = 0x1000;

    #[inline]
    fn supervisor_context(cpu : usize) -> usize { cpu * 2 + 1 }

    #[inline]
    unsafe fn read32(address : usize) -> u32 {
        unsafe { core::ptr::read_volatile(address as *const u32) }
    }

    #[inline]
    unsafe fn write32(address : usize, value : u32) {
        unsafe { core::ptr::write_volatile(address as *mut u32, value) }
    }

    fn validate(irq : u32) -> Result<(), ExternalIrqError> {
        if irq == 0 || irq > PLIC_MAX_IRQ {
            Err(ExternalIrqError)
        } else {
            Ok(())
        }
    }

    pub fn init_current_cpu() -> Result<(), ExternalIrqError> {
        let cpu = crate::arch::cpu::current_cpu_id().raw();
        let context = supervisor_context(cpu);
        unsafe {
            write32(CONTEXT_BASE + context * CONTEXT_STRIDE,
                    0)
        };
        crate::arch::interrupt::enable_external_interrupt();
        Ok(())
    }

    pub fn set_enabled(irq : u32, cpu : usize, enabled : bool) -> Result<(), ExternalIrqError> {
        validate(irq)?;
        let context = supervisor_context(cpu);
        let address = ENABLE_BASE + context * ENABLE_STRIDE + (irq as usize / 32) * 4;
        let bit = 1u32 << (irq % 32);
        unsafe {
            let old = read32(address);
            write32(address,
                    if enabled { old | bit } else { old & !bit });
            if enabled {
                write32(PRIORITY_BASE + irq as usize * 4, 1);
            }
        }
        Ok(())
    }

    pub fn is_enabled(irq : u32, cpu : usize) -> Result<bool, ExternalIrqError> {
        validate(irq)?;
        let context = supervisor_context(cpu);
        let address = ENABLE_BASE + context * ENABLE_STRIDE + (irq as usize / 32) * 4;
        Ok(unsafe { read32(address) } & (1u32 << (irq % 32)) != 0)
    }

    pub fn is_pending(irq : u32) -> Result<bool, ExternalIrqError> {
        validate(irq)?;
        let address = PENDING_BASE + (irq as usize / 32) * 4;
        Ok(unsafe { read32(address) } & (1u32 << (irq % 32)) != 0)
    }

    pub fn claim(cpu : usize) -> Option<u32> {
        let context = supervisor_context(cpu);
        let irq = unsafe { read32(CONTEXT_BASE + context * CONTEXT_STRIDE + 4) };
        (irq != 0).then_some(irq)
    }

    pub fn complete(cpu : usize, irq : u32) {
        let context = supervisor_context(cpu);
        unsafe {
            write32(CONTEXT_BASE + context * CONTEXT_STRIDE + 4,
                    irq)
        };
    }
}

#[cfg(feature = "impl-qemu-riscv64-opensbi")]
pub use active::*;

#[cfg(not(feature = "impl-qemu-riscv64-opensbi"))]
mod unsupported {
    use super::ExternalIrqError;
    pub fn init_current_cpu() -> Result<(), ExternalIrqError> { Err(ExternalIrqError) }
    pub fn set_enabled(_ : u32, _ : usize, _ : bool) -> Result<(), ExternalIrqError> {
        Err(ExternalIrqError)
    }
    pub fn is_enabled(_ : u32, _ : usize) -> Result<bool, ExternalIrqError> {
        Err(ExternalIrqError)
    }
    pub fn is_pending(_ : u32) -> Result<bool, ExternalIrqError> { Err(ExternalIrqError) }
    pub fn claim(_ : usize) -> Option<u32> { None }
    pub fn complete(_ : usize, _ : u32) {}
}

#[cfg(not(feature = "impl-qemu-riscv64-opensbi"))]
pub use unsupported::*;

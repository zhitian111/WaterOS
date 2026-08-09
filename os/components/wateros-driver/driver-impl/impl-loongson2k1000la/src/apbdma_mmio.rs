//! APBDMA order-register access for Loongson 2K1000LA.
//!
//! Linux uses its `lo_hi_readq`/`lo_hi_writeq` helpers for this 8-byte window:
//! the low 32-bit word is accessed first, followed by the high word. This module
//! independently models that observable ordering without importing GPL source.
//!
//! `UNVERIFIED_ON_HARDWARE`: volatile accessibility, little-endian MMIO behavior
//! and the controller's response to a failed second-half write require a board.

use core::sync::atomic::{Ordering, fence};

#[cfg(target_arch = "loongarch64")]
use crate::apbdma::Executor;
use crate::apbdma::{ExecutorError, OrderIo, OrderWriteFailure, WriteEffect};
#[cfg(target_arch = "loongarch64")]
use crate::irq_binding::InterruptBinding;
#[cfg(target_arch = "loongarch64")]
use crate::topology::DmaControllerDescription;

const LOW_OFFSET : usize = 0;
const HIGH_OFFSET : usize = 4;
#[cfg(target_arch = "loongarch64")]
const ORDER_WINDOW_SIZE : usize = 8;

/// One ordered 32-bit MMIO access. Implementations must not merge adjacent
/// calls into a native 64-bit transaction. A failed `write32` must mean that
/// this individual 32-bit write did not reach the device.
pub trait OrderMmio32 {
    fn read32(&mut self, offset : usize) -> Result<u32, ExecutorError>;
    fn write32(&mut self, offset : usize, value : u32) -> Result<(), ExecutorError>;
    /// Platform-specific proof that the DMA engine no longer accesses memory.
    /// The published Linux driver provides no such register-level probe.
    fn confirm_stopped(&mut self) -> Result<bool, ExecutorError> {
        Err(ExecutorError::StopUnverified)
    }
}

/// Non-atomic low-word/high-word adapter used by the APBDMA executor.
pub struct LoHiOrderIo<M> {
    mmio : M,
}

impl<M> LoHiOrderIo<M> {
    pub const fn new(mmio : M) -> Self { Self { mmio } }
    pub fn into_inner(self) -> M { self.mmio }
}

impl<M : OrderMmio32> OrderIo for LoHiOrderIo<M> {
    fn read64(&mut self) -> Result<u64, ExecutorError> {
        fence(Ordering::SeqCst);
        let low = self.mmio.read32(LOW_OFFSET)?;
        let high = self.mmio.read32(HIGH_OFFSET)?;
        fence(Ordering::SeqCst);
        Ok(low as u64 | (high as u64) << 32)
    }

    fn write64(&mut self, value : u64) -> Result<(), OrderWriteFailure> {
        fence(Ordering::SeqCst);
        self.mmio.write32(LOW_OFFSET, value as u32)
                 .map_err(|error| OrderWriteFailure { error,
                                                      effect : WriteEffect::Untouched })?;
        self.mmio.write32(HIGH_OFFSET, (value >> 32) as u32)
                 .map_err(|error| OrderWriteFailure { error,
                                                      effect : WriteEffect::MayHaveWritten })?;
        fence(Ordering::SeqCst);
        Ok(())
    }

    fn confirm_stopped(&mut self) -> Result<bool, ExecutorError> {
        self.mmio.confirm_stopped()
    }
}

#[cfg(target_arch = "loongarch64")]
pub type PlatformExecutor = Executor<LoHiOrderIo<VolatileOrderMmio32>>;

/// Assemble an executor only from a topology-validated controller window.
///
/// The caller must additionally keep the controller clock enabled and arrange
/// IRQ acknowledgement before using the returned executor.
#[cfg(target_arch = "loongarch64")]
pub unsafe fn executor_from_controller(controller : &DmaControllerDescription,
                                       binding : InterruptBinding)
                                       -> Result<PlatformExecutor, ExecutorError> {
    if binding.provider_phandle() != controller.interrupt.parent_phandle ||
       binding.global_irq().local() != controller.interrupt.cells[0]
    {
        return Err(ExecutorError::UnexpectedIrq);
    }
    Ok(Executor::new(unsafe { VolatileOrderMmio32::from_controller(controller)? },
                     binding.global_irq()))
}

/// Raw volatile 32-bit access to a topology-validated APBDMA order window.
#[cfg(target_arch = "loongarch64")]
pub struct VolatileOrderMmio32 {
    base : *mut u8,
}

#[cfg(target_arch = "loongarch64")]
impl VolatileOrderMmio32 {
    /// Build from discovered resources. The caller must ensure the physical MMIO
    /// range is mapped into the active kernel address space for this lifetime.
    pub unsafe fn from_controller(controller : &DmaControllerDescription)
                                  -> Result<LoHiOrderIo<Self>, ExecutorError> {
        let base = controller.mmio.base;
        if controller.mmio.size != ORDER_WINDOW_SIZE || base == 0 || base % 4 != 0 {
            return Err(ExecutorError::Register);
        }
        Ok(LoHiOrderIo::new(Self { base : base as *mut u8 }))
    }

    fn register(&self, offset : usize) -> Result<*mut u32, ExecutorError> {
        if offset != LOW_OFFSET && offset != HIGH_OFFSET {
            return Err(ExecutorError::Register);
        }
        Ok(unsafe { self.base.add(offset).cast::<u32>() })
    }
}

#[cfg(target_arch = "loongarch64")]
impl OrderMmio32 for VolatileOrderMmio32 {
    fn read32(&mut self, offset : usize) -> Result<u32, ExecutorError> {
        Ok(unsafe { core::ptr::read_volatile(self.register(offset)?) })
    }

    fn write32(&mut self, offset : usize, value : u32) -> Result<(), ExecutorError> {
        unsafe { core::ptr::write_volatile(self.register(offset)?, value) };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Event {
        Read(usize),
        Write(usize, u32),
    }

    struct MockMmio32 {
        low : u32,
        high : u32,
        fail_high_write : bool,
        events : Vec<Event>,
    }

    impl OrderMmio32 for MockMmio32 {
        fn read32(&mut self, offset : usize) -> Result<u32, ExecutorError> {
            self.events.push(Event::Read(offset));
            match offset {
                LOW_OFFSET => Ok(self.low),
                HIGH_OFFSET => Ok(self.high),
                _ => Err(ExecutorError::Register),
            }
        }

        fn write32(&mut self, offset : usize, value : u32) -> Result<(), ExecutorError> {
            self.events.push(Event::Write(offset, value));
            if offset == HIGH_OFFSET && self.fail_high_write {
                return Err(ExecutorError::Register);
            }
            match offset {
                LOW_OFFSET => self.low = value,
                HIGH_OFFSET => self.high = value,
                _ => return Err(ExecutorError::Register),
            }
            Ok(())
        }
    }

    fn mock(low : u32, high : u32) -> MockMmio32 {
        MockMmio32 { low,
                     high,
                     fail_high_write : false,
                     events : Vec::new() }
    }

    #[test]
    fn reads_low_then_high_and_reconstructs_value() {
        let mut io = LoHiOrderIo::new(mock(0x89ab_cdef, 0x0123_4567));
        assert_eq!(io.read64(), Ok(0x0123_4567_89ab_cdef));
        assert_eq!(io.into_inner().events,
                   [Event::Read(LOW_OFFSET), Event::Read(HIGH_OFFSET)]);
    }

    #[test]
    fn writes_low_then_high_without_native_64_bit_access() {
        let mut io = LoHiOrderIo::new(mock(0, 0));
        io.write64(0x0123_4567_89ab_cdef).unwrap();
        let mmio = io.into_inner();
        assert_eq!((mmio.low, mmio.high), (0x89ab_cdef, 0x0123_4567));
        assert_eq!(mmio.events,
                   [Event::Write(LOW_OFFSET, 0x89ab_cdef),
                    Event::Write(HIGH_OFFSET, 0x0123_4567)]);
    }

    #[test]
    fn reports_second_half_write_failure_without_hiding_partial_write() {
        let mut mmio = mock(0xaaaa_aaaa, 0xbbbb_bbbb);
        mmio.fail_high_write = true;
        let mut io = LoHiOrderIo::new(mmio);
        assert_eq!(io.write64(0x0123_4567_89ab_cdef),
                   Err(OrderWriteFailure { error : ExecutorError::Register,
                                           effect : WriteEffect::MayHaveWritten }));
        let mmio = io.into_inner();
        assert_eq!(mmio.low, 0x89ab_cdef);
        assert_eq!(mmio.high, 0xbbbb_bbbb);
        assert_eq!(mmio.events,
                   [Event::Write(LOW_OFFSET, 0x89ab_cdef),
                    Event::Write(HIGH_OFFSET, 0x0123_4567)]);
    }

    #[test]
    fn refuses_to_invent_stop_confirmation_without_platform_evidence() {
        let mut io = LoHiOrderIo::new(mock(0, 0));
        assert_eq!(io.confirm_stopped(), Err(ExecutorError::StopUnverified));
    }
}

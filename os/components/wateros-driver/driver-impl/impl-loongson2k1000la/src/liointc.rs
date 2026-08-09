//! LIOINTC 2.0 register model.
//!
//! Register offsets follow the upstream Linux irqchip driver and the Loongson
//! 2K documentation. The volatile backend is `UNVERIFIED_ON_HARDWARE`.

use api_v0::{DriverError, DriverResult};
use crate::irq_domain::{AcknowledgedIrq, GlobalIrq};

pub const IRQ_COUNT : u32 = 32;
pub const MAX_CORES : usize = 4;
pub const MAIN_REGISTER_BYTES : usize = 0x40;

const ENABLE_STATUS : usize = 0x24;
const ENABLE_SET : usize = 0x28;
const ENABLE_CLEAR : usize = 0x2C;
const POLARITY : usize = 0x30;
const EDGE : usize = 0x34;

pub trait RegisterIo {
    fn read32(&self, address : usize) -> u32;
    fn write32(&mut self, address : usize, value : u32);
    fn write8(&mut self, address : usize, value : u8);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    LevelHigh,
    LevelLow,
    EdgeRising,
    EdgeFalling,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Route {
    pub core_mask : u8,
    pub parent_line : u8,
}

impl Route {
    pub fn encode(self) -> DriverResult<u8> {
        if self.core_mask == 0 || self.core_mask & !0x0F != 0 || self.parent_line >= 4 {
            return Err(DriverError::InvalidParam);
        }
        Ok(self.core_mask | (1 << (4 + self.parent_line)))
    }
}

pub struct LioIntc<I> {
    io : I,
    main_base : usize,
    core_isr : [Option<usize>; MAX_CORES],
}

impl<I : RegisterIo> LioIntc<I> {
    pub fn new(io : I, main_base : usize, core_isr : &[usize]) -> DriverResult<Self> {
        if main_base.checked_add(MAIN_REGISTER_BYTES)
                    .is_none() ||
           core_isr.is_empty() ||
           core_isr.len() > MAX_CORES
        {
            return Err(DriverError::InvalidParam);
        }
        let mut isr = [None; MAX_CORES];
        for (slot, address) in isr.iter_mut()
                                  .zip(core_isr.iter()
                                               .copied())
        {
            if address.checked_add(4)
                      .is_none()
            {
                return Err(DriverError::InvalidParam);
            }
            *slot = Some(address);
        }
        Ok(Self { io,
                  main_base,
                  core_isr : isr })
    }

    fn irq_mask(irq : u32) -> DriverResult<u32> {
        if irq >= IRQ_COUNT {
            return Err(DriverError::InvalidParam);
        }
        Ok(1u32 << irq)
    }

    pub fn disable_all(&mut self) {
        self.io
            .write32(self.main_base + ENABLE_CLEAR, u32::MAX);
    }

    pub fn enable(&mut self, irq : u32) -> DriverResult<()> {
        self.io
            .write32(self.main_base + ENABLE_SET,
                     Self::irq_mask(irq)?);
        Ok(())
    }

    /// Masks the source. This also clears a latched pulse; a level source must
    /// still be cleared at its originating device.
    pub fn mask_ack(&mut self, irq : u32) -> DriverResult<()> {
        self.io
            .write32(self.main_base + ENABLE_CLEAR,
                     Self::irq_mask(irq)?);
        Ok(())
    }

    /// Mask/ack one local source and return evidence tied to its global bank.
    /// A level-triggered device must still clear its own interrupt condition.
    pub fn mask_ack_claim(&mut self, bank : usize, irq : u32)
                          -> DriverResult<AcknowledgedIrq> {
        let global = GlobalIrq::from_bank_local(bank, irq)
                               .map_err(|_| DriverError::InvalidParam)?;
        self.mask_ack(irq)?;
        Ok(AcknowledgedIrq::after_mask_ack(global))
    }

    pub fn configure_route(&mut self, irq : u32, route : Route) -> DriverResult<()> {
        if irq >= IRQ_COUNT {
            return Err(DriverError::InvalidParam);
        }
        self.io
            .write8(self.main_base + irq as usize,
                    route.encode()?);
        Ok(())
    }

    pub fn set_trigger(&mut self, irq : u32, trigger : Trigger) -> DriverResult<()> {
        let mask = Self::irq_mask(irq)?;
        let mut edge = self.io
                           .read32(self.main_base + EDGE);
        let mut polarity = self.io
                               .read32(self.main_base + POLARITY);
        match trigger {
            Trigger::LevelHigh => {
                edge &= !mask;
                polarity &= !mask;
            }
            Trigger::LevelLow => {
                edge &= !mask;
                polarity |= mask;
            }
            Trigger::EdgeRising => {
                edge |= mask;
                polarity &= !mask;
            }
            Trigger::EdgeFalling => {
                edge |= mask;
                polarity |= mask;
            }
        }
        self.io
            .write32(self.main_base + EDGE, edge);
        self.io
            .write32(self.main_base + POLARITY, polarity);
        Ok(())
    }

    pub fn pending(&self, core : usize) -> DriverResult<u32> {
        let address = self.core_isr
                          .get(core)
                          .copied()
                          .flatten()
                          .ok_or(DriverError::InvalidParam)?;
        Ok(self.io
               .read32(address))
    }

    pub fn claim_first(&self, core : usize) -> DriverResult<Option<u32>> {
        let pending = self.pending(core)? &
                      self.io
                          .read32(self.main_base + ENABLE_STATUS);
        Ok((pending != 0).then(|| pending.trailing_zeros()))
    }

    pub fn into_inner(self) -> I { self.io }
}

pub struct VolatileMmio;

impl RegisterIo for VolatileMmio {
    fn read32(&self, address : usize) -> u32 {
        // SAFETY: The caller owns the validated MMIO mapping contract.
        unsafe { core::ptr::read_volatile(address as *const u32) }
    }

    fn write32(&mut self, address : usize, value : u32) {
        // SAFETY: The caller owns the validated MMIO mapping contract.
        unsafe { core::ptr::write_volatile(address as *mut u32, value) }
    }

    fn write8(&mut self, address : usize, value : u8) {
        // SAFETY: The caller owns the validated MMIO mapping contract.
        unsafe { core::ptr::write_volatile(address as *mut u8, value) }
    }
}

#[cfg(test)]
mod tests {
    use alloc::vec::Vec;

    use super::*;

    #[derive(Default)]
    struct ModelIo {
        registers : Vec<(usize, u32)>,
        writes32 : Vec<(usize, u32)>,
        writes8 : Vec<(usize, u8)>,
    }

    impl ModelIo {
        fn set(&mut self, address : usize, value : u32) {
            self.registers
                .push((address, value));
        }
    }

    impl RegisterIo for ModelIo {
        fn read32(&self, address : usize) -> u32 {
            self.registers
                .iter()
                .rev()
                .find(|(candidate, _)| *candidate == address)
                .map(|(_, value)| *value)
                .unwrap_or(0)
        }

        fn write32(&mut self, address : usize, value : u32) {
            self.writes32
                .push((address, value));
            self.set(address, value);
        }

        fn write8(&mut self, address : usize, value : u8) {
            self.writes8
                .push((address, value));
        }
    }

    const BASE : usize = 0x1000;
    const ISR0 : usize = 0x2000;
    const ISR1 : usize = 0x3000;

    fn controller() -> LioIntc<ModelIo> {
        LioIntc::new(ModelIo::default(), BASE, &[ISR0, ISR1]).unwrap()
    }

    #[test]
    fn encodes_route_and_rejects_invalid_fields() {
        assert_eq!(Route { core_mask : 0b0101,
                           parent_line : 2 }.encode(),
                   Ok(0x45));
        assert_eq!(Route { core_mask : 0,
                           parent_line : 0 }.encode(),
                   Err(DriverError::InvalidParam));
        assert_eq!(Route { core_mask : 1,
                           parent_line : 4 }.encode(),
                   Err(DriverError::InvalidParam));
    }

    #[test]
    fn writes_route_enable_and_disable_registers() {
        let mut controller = controller();
        controller.configure_route(31, Route { core_mask : 2,
                                               parent_line : 1 })
                  .unwrap();
        controller.enable(31)
                  .unwrap();
        controller.mask_ack(31)
                  .unwrap();
        controller.disable_all();
        let io = controller.into_inner();
        assert_eq!(io.writes8, &[(BASE + 31, 0x22)]);
        assert_eq!(io.writes32, &[(BASE + ENABLE_SET,
                                   1 << 31),
                                  (BASE + ENABLE_CLEAR,
                                   1 << 31),
                                  (BASE + ENABLE_CLEAR,
                                   u32::MAX)]);
    }

    #[test]
    fn mask_ack_claim_binds_evidence_to_global_irq() {
        let mut valid_controller = controller();
        let acknowledged = valid_controller.mask_ack_claim(1, 13).unwrap();
        assert_eq!(acknowledged.irq(), GlobalIrq::from_bank_local(1, 13).unwrap());
        assert_eq!(valid_controller.into_inner().writes32,
                   &[(BASE + ENABLE_CLEAR, 1 << 13)]);

        let mut invalid_controller = controller();
        assert_eq!(invalid_controller.mask_ack_claim(2, 13),
                   Err(DriverError::InvalidParam));
        assert!(invalid_controller.into_inner().writes32.is_empty());
    }

    #[test]
    fn programs_all_four_trigger_combinations() {
        let mut controller = controller();
        controller.set_trigger(3, Trigger::EdgeFalling)
                  .unwrap();
        controller.set_trigger(4, Trigger::LevelHigh)
                  .unwrap();
        controller.set_trigger(5, Trigger::LevelLow)
                  .unwrap();
        controller.set_trigger(6, Trigger::EdgeRising)
                  .unwrap();
        let io = controller.into_inner();
        assert_eq!(io.read32(BASE + EDGE),
                   (1 << 3) | (1 << 6));
        assert_eq!(io.read32(BASE + POLARITY),
                   (1 << 3) | (1 << 5));
    }

    #[test]
    fn claims_lowest_enabled_pending_irq_per_core() {
        let mut io = ModelIo::default();
        io.set(ISR0, (1 << 2) | (1 << 7));
        io.set(ISR1, 1 << 12);
        io.set(BASE + ENABLE_STATUS,
               (1 << 7) | (1 << 12));
        let controller = LioIntc::new(io, BASE, &[ISR0, ISR1]).unwrap();
        assert_eq!(controller.claim_first(0), Ok(Some(7)));
        assert_eq!(controller.claim_first(1), Ok(Some(12)));
        assert_eq!(controller.claim_first(2),
                   Err(DriverError::InvalidParam));
    }

    #[test]
    fn rejects_out_of_range_irq_without_writes() {
        let mut controller = controller();
        assert_eq!(controller.enable(IRQ_COUNT),
                   Err(DriverError::InvalidParam));
        assert_eq!(controller.configure_route(IRQ_COUNT, Route { core_mask : 1,
                                                                 parent_line : 0 }),
                   Err(DriverError::InvalidParam));
        let io = controller.into_inner();
        assert!(io.writes32
                  .is_empty());
        assert!(io.writes8
                  .is_empty());
    }
}

use alloc::vec::Vec;
use api_v0::{DriverError, DriverResult, MmioRegion};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextInterrupt {
    pub interrupt_controller : u32,
    pub interrupt : u32,
    /// Hardware hart id owning `interrupt_controller`; resolved from `/cpus`.
    pub hart_id : Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlicDescription {
    pub mmio : MmioRegion,
    pub sources : u32,
    pub contexts : Vec<ContextInterrupt>,
}

pub fn parse_contexts(raw : &[u8]) -> DriverResult<Vec<ContextInterrupt>> {
    if raw.len() % 8 != 0 {
        return Err(DriverError::InvalidDtb);
    }
    Ok(raw.chunks_exact(8)
          .map(|pair| ContextInterrupt { interrupt_controller:
                                             u32::from_be_bytes(pair[0..4].try_into()
                                                                          .unwrap()),
                                         interrupt : u32::from_be_bytes(pair[4..8].try_into()
                                                                                  .unwrap()),
                                         hart_id : None })
          .collect())
}

/// Standard RISC-V PLIC register arithmetic. Construction does not touch MMIO.
pub struct PlicMmio {
    description : PlicDescription,
    context : usize,
}

impl PlicMmio {
    pub fn new(description : PlicDescription, context : usize) -> DriverResult<Self> {
        if context >=
           description.contexts
                      .len() ||
           description.sources == 0
        {
            return Err(DriverError::InvalidParam);
        }
        let end = 0x20_0004usize.checked_add(context.checked_mul(0x1000)
                                                    .ok_or(DriverError::InvalidParam)?)
                                .ok_or(DriverError::InvalidParam)?;
        if description.mmio
                      .size <
           end + 4
        {
            return Err(DriverError::InvalidParam);
        }
        Ok(Self { description,
                  context })
    }
    pub fn claim_complete_offset(&self) -> usize { 0x20_0004 + self.context * 0x1000 }
    pub fn threshold_offset(&self) -> usize { 0x20_0000 + self.context * 0x1000 }
    pub fn priority_offset(&self, source : u32) -> DriverResult<usize> {
        self.validate_source(source)?;
        (source as usize).checked_mul(4).ok_or(DriverError::InvalidParam)
    }
    pub fn enable_offset(&self, source : u32) -> DriverResult<usize> {
        self.validate_source(source)?;
        Ok(0x2000 + self.context * 0x80 + source as usize / 32 * 4)
    }
    fn validate_source(&self, source : u32) -> DriverResult<()> {
        if source == 0 || source > self.description.sources {
            Err(DriverError::InvalidParam)
        } else {
            Ok(())
        }
    }
    fn register_ptr(&self, offset : usize) -> DriverResult<*mut u32> {
        let end = offset.checked_add(4).ok_or(DriverError::InvalidParam)?;
        if end > self.description.mmio.size {
            return Err(DriverError::InvalidParam);
        }
        let address = self.description.mmio.base.checked_add(offset)
                                                   .ok_or(DriverError::InvalidParam)?;
        Ok(address as *mut u32)
    }
    /// # Safety
    /// The PLIC region must be mapped and this instance's context must belong to
    /// the calling hart.
    pub unsafe fn configure_source(&self, source : u32, priority : u32) -> DriverResult<()> {
        let priority_ptr = self.register_ptr(self.priority_offset(source)?)?;
        let enable_ptr = self.register_ptr(self.enable_offset(source)?)?;
        let bit = 1u32 << (source % 32);
        unsafe {
            core::ptr::write_volatile(priority_ptr, priority);
            let enabled = core::ptr::read_volatile(enable_ptr);
            core::ptr::write_volatile(enable_ptr, enabled | bit);
        }
        Ok(())
    }
    /// # Safety
    /// The PLIC region must be mapped and this instance's context must belong to
    /// the calling hart.
    pub unsafe fn disable_source(&self, source : u32) -> DriverResult<()> {
        let enable_ptr = self.register_ptr(self.enable_offset(source)?)?;
        let bit = 1u32 << (source % 32);
        unsafe {
            let enabled = core::ptr::read_volatile(enable_ptr);
            core::ptr::write_volatile(enable_ptr, enabled & !bit);
        }
        Ok(())
    }
    /// # Safety
    /// The PLIC region must be mapped and this instance's context must belong to
    /// the calling hart.
    pub unsafe fn set_threshold(&self, threshold : u32) -> DriverResult<()> {
        let ptr = self.register_ptr(self.threshold_offset())?;
        unsafe { core::ptr::write_volatile(ptr, threshold) };
        Ok(())
    }
    /// # Safety
    /// Caller must have confirmed the DTB context mapping and mapped the PLIC MMIO region.
    pub unsafe fn claim(&self) -> DriverResult<u32> {
        let ptr = self.register_ptr(self.claim_complete_offset())?;
        Ok(unsafe { core::ptr::read_volatile(ptr) })
    }
    /// # Safety
    /// `source` must be a value previously returned by this PLIC and MMIO must be mapped.
    pub unsafe fn complete(&self, source : u32) -> DriverResult<()> {
        self.validate_source(source)?;
        let ptr = self.register_ptr(self.claim_complete_offset())
                      .map_err(|_| DriverError::InvalidParam)?;
        unsafe { core::ptr::write_volatile(ptr, source) };
        Ok(())
    }
}

impl PlicDescription {
    pub fn context_for_hart(&self, hart_id : usize) -> Option<usize> {
        let mut found = None;
        for (index, context) in self.contexts.iter().enumerate() {
            if context.interrupt == 9 && context.hart_id == Some(hart_id) {
                if found.is_some() {
                    return None;
                }
                found = Some(index);
            }
        }
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_context_pairs() {
        let raw = [0, 0, 0, 7, 0, 0, 0, 9, 0, 0, 0, 8, 0, 0, 0, 11];
        let parsed = parse_contexts(&raw).unwrap();
        assert_eq!(parsed,
                   alloc::vec![ContextInterrupt { interrupt_controller : 7,
                                                  interrupt : 9,
                                                  hart_id : None },
                               ContextInterrupt { interrupt_controller : 8,
                                                  interrupt : 11,
                                                  hart_id : None }]);
        assert!(parse_contexts(&raw[..12]).is_err());
    }
    #[test]
    fn validates_standard_offsets() {
        let d = PlicDescription { mmio : MmioRegion { base : 0xC00_0000,
                                                      size : 0x40_0000 },
                                  sources : 136,
                                  contexts:
                                      alloc::vec![ContextInterrupt { interrupt_controller : 1,
                                                                     interrupt : 9,
                                                                     hart_id : Some(0) }] };
        let plic = PlicMmio::new(d, 0).unwrap();
        assert_eq!(plic.enable_offset(32)
                       .unwrap(),
                   0x2004);
        assert_eq!(plic.claim_complete_offset(), 0x20_0004);
        assert!(plic.enable_offset(137)
                    .is_err());
    }

    #[test]
    fn selects_one_supervisor_context_and_rejects_ambiguity() {
        let context = |interrupt : u32, hart_id : Option<usize>| ContextInterrupt {
            interrupt_controller : 1,
            interrupt,
            hart_id,
        };
        let description = PlicDescription { mmio : MmioRegion { base : 0xC00_0000,
                                                                  size : 0x40_0000 },
                                              sources : 64,
                                              contexts : alloc::vec![context(11, Some(0)),
                                                                     context(9, Some(1)),
                                                                     context(9, None)] };
        assert_eq!(description.context_for_hart(0), None);
        assert_eq!(description.context_for_hart(1), Some(1));
        assert_eq!(description.context_for_hart(2), None);

        let mut duplicate = description.clone();
        duplicate.contexts.push(context(9, Some(1)));
        assert_eq!(duplicate.context_for_hart(1), None);
    }
    #[test]
    fn exercises_register_io_against_memory() {
        let mut words = alloc::vec![0u32; 0x20_1000 / 4];
        let description = PlicDescription {
            mmio : MmioRegion { base : words.as_mut_ptr() as usize,
                                size : words.len() * 4 },
            sources : 64,
            contexts : alloc::vec![ContextInterrupt { interrupt_controller : 1,
                                                       interrupt : 9,
                                                       hart_id : Some(0) }],
        };
        let plic = PlicMmio::new(description, 0).unwrap();
        unsafe {
            plic.configure_source(33, 3).unwrap();
            plic.set_threshold(1).unwrap();
            plic.disable_source(33).unwrap();
        }
        assert_eq!(words[33], 3);
        assert_eq!(words[0x2004 / 4], 0);
        assert_eq!(words[0x20_0000 / 4], 1);
    }
}

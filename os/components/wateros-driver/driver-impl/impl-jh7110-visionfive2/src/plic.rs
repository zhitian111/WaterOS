use alloc::vec::Vec;
use api_v0::{DriverError, DriverResult, MmioRegion};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextInterrupt {
    pub interrupt_controller : u32,
    pub interrupt : u32,
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
                                                                                  .unwrap()) })
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
    pub fn enable_offset(&self, source : u32) -> DriverResult<usize> {
        if source == 0 ||
           source >
           self.description
               .sources
        {
            return Err(DriverError::InvalidParam);
        }
        Ok(0x2000 + self.context * 0x80 + source as usize / 32 * 4)
    }
    /// # Safety
    /// Caller must have confirmed the DTB context mapping and mapped the PLIC MMIO region.
    pub unsafe fn claim(&self) -> u32 {
        unsafe {
            core::ptr::read_volatile((self.description
                                          .mmio
                                          .base +
                                      self.claim_complete_offset())
                                     as *const u32)
        }
    }
    /// # Safety
    /// `source` must be a value previously returned by this PLIC and MMIO must be mapped.
    pub unsafe fn complete(&self, source : u32) {
        unsafe {
            core::ptr::write_volatile((self.description
                                           .mmio
                                           .base +
                                       self.claim_complete_offset())
                                      as *mut u32,
                                      source)
        }
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
                                                  interrupt : 9 },
                               ContextInterrupt { interrupt_controller : 8,
                                                  interrupt : 11 }]);
        assert!(parse_contexts(&raw[..12]).is_err());
    }
    #[test]
    fn validates_standard_offsets() {
        let d = PlicDescription { mmio : MmioRegion { base : 0xC00_0000,
                                                      size : 0x40_0000 },
                                  sources : 136,
                                  contexts:
                                      alloc::vec![ContextInterrupt { interrupt_controller : 1,
                                                                     interrupt : 9 }] };
        let plic = PlicMmio::new(d, 0).unwrap();
        assert_eq!(plic.enable_offset(32)
                       .unwrap(),
                   0x2004);
        assert_eq!(plic.claim_complete_offset(), 0x20_0004);
        assert!(plic.enable_offset(137)
                    .is_err());
    }
}

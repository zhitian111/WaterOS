//! Read-only LS2K1000 GPIO card-detect diagnostics.
//!
//! The model follows Linux's bit-control register layout but never changes
//! direction, output, mux or interrupt state. Physical register behavior and
//! board wiring remain `UNVERIFIED_ON_HARDWARE`.

use crate::topology::{GpioLineDescription, GpioProvider};

const DIRECTION_OFFSET : usize = 0x00;
const INPUT_OFFSET : usize = 0x20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpioError {
    Io,
    UnsupportedProvider,
    PinOutOfRange,
    NotInput,
}

pub trait RegisterIo {
    fn read64(&mut self, offset : usize) -> Result<u64, GpioError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CardDetectSnapshot {
    pub direction_raw : u64,
    pub input_raw : u64,
    pub pin : u8,
    pub active_low : bool,
    pub level_high : bool,
    pub card_present : bool,
}

pub fn card_detect_snapshot<R : RegisterIo>(line : &GpioLineDescription,
                                            registers : &mut R)
                                            -> Result<CardDetectSnapshot, GpioError> {
    let ngpios = match line.provider {
        GpioProvider::Loongson2k1000 { ngpios, .. } => ngpios,
        GpioProvider::Unsupported { .. } => return Err(GpioError::UnsupportedProvider),
    };
    if line.pin >= ngpios || line.pin >= 64 {
        return Err(GpioError::PinOutOfRange);
    }
    let direction_raw = registers.read64(DIRECTION_OFFSET)?;
    let mask = 1u64 << line.pin;
    if direction_raw & mask == 0 {
        return Err(GpioError::NotInput);
    }
    let input_raw = registers.read64(INPUT_OFFSET)?;
    let level_high = input_raw & mask != 0;
    Ok(CardDetectSnapshot { direction_raw,
                            input_raw,
                            pin : line.pin,
                            active_low : line.active_low,
                            level_high,
                            card_present : level_high ^ line.active_low })
}

/// Volatile read-only backend for a topology-validated LS2K1000 GPIO window.
#[cfg(target_arch = "loongarch64")]
pub struct VolatileRegisters {
    base : *const u8,
    size : usize,
}

#[cfg(target_arch = "loongarch64")]
impl VolatileRegisters {
    /// The caller must exclusively own a valid mapped GPIO window. This does
    /// not read hardware or alter the pin configuration.
    pub unsafe fn new(base : usize, size : usize) -> Result<Self, GpioError> {
        if base == 0 || base % 8 != 0 || size < 0x28 {
            return Err(GpioError::Io);
        }
        Ok(Self { base : base as *const u8,
                  size })
    }

    fn contains(&self, offset : usize) -> bool {
        offset.checked_add(8)
              .is_some_and(|end| end <= self.size)
    }
}

#[cfg(target_arch = "loongarch64")]
impl RegisterIo for VolatileRegisters {
    fn read64(&mut self, offset : usize) -> Result<u64, GpioError> {
        if offset % 8 != 0 || !self.contains(offset) {
            return Err(GpioError::Io);
        }
        // SAFETY: upheld by new's caller; range and alignment checked above.
        Ok(unsafe {
            core::ptr::read_volatile(self.base
                                         .add(offset)
                                         .cast::<u64>())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::topology::ResourceSpecifier;
    use alloc::vec;

    struct Model {
        direction : u64,
        input : u64,
        fail_at : Option<usize>,
        reads : alloc::vec::Vec<usize>,
    }

    impl RegisterIo for Model {
        fn read64(&mut self, offset : usize) -> Result<u64, GpioError> {
            self.reads
                .push(offset);
            if self.fail_at == Some(offset) {
                Err(GpioError::Io)
            } else if offset == DIRECTION_OFFSET {
                Ok(self.direction)
            } else {
                Ok(self.input)
            }
        }
    }

    fn line(active_low : bool) -> GpioLineDescription {
        GpioLineDescription {
            specifier : ResourceSpecifier { provider_phandle : 5,
                                            args : vec![22, active_low as u32] },
            provider : GpioProvider::Loongson2k1000 {
                mmio : api_v0::MmioRegion { base : 0x1fe0_0500, size : 0x38 },
                ngpios : 64,
            },
            pin : 22,
            active_low,
        }
    }

    #[test]
    fn samples_direction_then_input_and_applies_active_low() {
        let mask = 1 << 22;
        let mut io = Model { direction : mask,
                             input : 0,
                             fail_at : None,
                             reads : vec![] };
        let state = card_detect_snapshot(&line(true), &mut io).unwrap();
        assert_eq!(io.reads, [0x00, 0x20]);
        assert!(!state.level_high);
        assert!(state.card_present);

        io.reads.clear();
        io.input = mask;
        assert!(!card_detect_snapshot(&line(true), &mut io).unwrap()
                                                           .card_present);
        assert!(card_detect_snapshot(&line(false), &mut io).unwrap()
                                                           .card_present);
    }

    #[test]
    fn refuses_output_pin_without_changing_direction() {
        let mut io = Model { direction : 0,
                             input : 1 << 22,
                             fail_at : None,
                             reads : vec![] };
        assert_eq!(card_detect_snapshot(&line(true), &mut io),
                   Err(GpioError::NotInput));
        assert_eq!(io.reads, [0x00]);
    }

    #[test]
    fn unsupported_or_out_of_range_provider_never_reads() {
        let mut io = Model { direction : u64::MAX,
                             input : 0,
                             fail_at : None,
                             reads : vec![] };
        let mut value = line(true);
        value.provider = GpioProvider::Unsupported { phandle : 5 };
        assert_eq!(card_detect_snapshot(&value, &mut io),
                   Err(GpioError::UnsupportedProvider));
        value.provider =
            GpioProvider::Loongson2k1000 { mmio : api_v0::MmioRegion { base : 0x1FE0_0500,
                                                                       size : 0x38 },
                                           ngpios : 16 };
        assert_eq!(card_detect_snapshot(&value, &mut io),
                   Err(GpioError::PinOutOfRange));
        assert!(io.reads.is_empty());
    }

    #[test]
    fn propagates_each_read_failure() {
        for offset in [DIRECTION_OFFSET,
                       INPUT_OFFSET]
        {
            let mut io = Model { direction : u64::MAX,
                                 input : 0,
                                 fail_at : Some(offset),
                                 reads : vec![] };
            assert_eq!(card_detect_snapshot(&line(true), &mut io),
                       Err(GpioError::Io));
        }
    }
}

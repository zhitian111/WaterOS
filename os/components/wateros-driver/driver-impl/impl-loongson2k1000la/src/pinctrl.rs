//! Read-only Loongson-2 MMC pinmux evidence.
//!
//! Linux mainline maps SDIO to bit 20 and the PWM2/GPIO22 card-detect mux to
//! bit 14 of the first pinctrl register. Physical semantics remain
//! `UNVERIFIED_ON_HARDWARE`; this module never writes the register.

use crate::topology::{MmcPinctrlDescription, PinctrlProvider};

const MUX_REGISTER : usize = 0;
const SDIO_BIT : u32 = 1 << 20;
const CARD_DETECT_GPIO_BIT : u32 = 1 << 14;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinctrlError {
    Io,
    Missing,
    UnsupportedProvider,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinctrlSnapshot {
    pub mux_raw : u32,
    pub sdio_selected : bool,
    pub card_detect_gpio_selected : bool,
}

pub trait RegisterIo {
    fn read32(&mut self, offset : usize) -> Result<u32, PinctrlError>;
}

pub fn snapshot(state : Option<MmcPinctrlDescription>,
                registers : &mut impl RegisterIo)
                -> Result<PinctrlSnapshot, PinctrlError> {
    match state.map(|state| state.provider) {
        None => Err(PinctrlError::Missing),
        Some(PinctrlProvider::Unsupported) => Err(PinctrlError::UnsupportedProvider),
        Some(PinctrlProvider::Loongson2k { .. }) => {
            let mux_raw = registers.read32(MUX_REGISTER)?;
            Ok(PinctrlSnapshot { mux_raw,
                                 sdio_selected : mux_raw & SDIO_BIT != 0,
                                 card_detect_gpio_selected : mux_raw & CARD_DETECT_GPIO_BIT == 0 })
        }
    }
}

#[cfg(target_arch = "loongarch64")]
pub struct VolatileRegisters {
    base : usize,
}

#[cfg(target_arch = "loongarch64")]
impl VolatileRegisters {
    /// # Safety
    /// `base..base + size` must be mapped device memory for the pin controller.
    pub unsafe fn new(base : usize, size : usize) -> Result<Self, PinctrlError> {
        if base == 0 || base % 4 != 0 || size < 4 {
            return Err(PinctrlError::Io);
        }
        Ok(Self { base })
    }
}

#[cfg(target_arch = "loongarch64")]
impl RegisterIo for VolatileRegisters {
    fn read32(&mut self, offset : usize) -> Result<u32, PinctrlError> {
        if offset != MUX_REGISTER {
            return Err(PinctrlError::Io);
        }
        // SAFETY: constructor and caller uphold the mapped-device-memory contract.
        Ok(unsafe { core::ptr::read_volatile((self.base + offset) as *const u32) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use api_v0::MmioRegion;

    #[derive(Default)]
    struct Model {
        raw : u32,
        reads : usize,
        fail : bool,
    }

    impl RegisterIo for Model {
        fn read32(&mut self, offset : usize) -> Result<u32, PinctrlError> {
            assert_eq!(offset, 0);
            self.reads += 1;
            if self.fail {
                Err(PinctrlError::Io)
            } else {
                Ok(self.raw)
            }
        }
    }

    fn state(provider : PinctrlProvider) -> Option<MmcPinctrlDescription> {
        Some(MmcPinctrlDescription { state_phandle : 1,
                                     provider })
    }

    #[test]
    fn decodes_sdio_and_active_low_gpio_mux_from_one_read() {
        let provider = PinctrlProvider::Loongson2k { mmio : MmioRegion { base : 0x1FE0_0420,
                                                                         size : 0x18 } };
        for (raw, sdio, gpio) in [(1 << 20, true, true),
                                  ((1 << 20) | (1 << 14), true, false),
                                  (0, false, true)]
        {
            let mut registers = Model { raw,
                                        ..Default::default() };
            let result = snapshot(state(provider), &mut registers).unwrap();
            assert_eq!((result.sdio_selected, result.card_detect_gpio_selected),
                       (sdio, gpio));
            assert_eq!(registers.reads, 1);
        }
    }

    #[test]
    fn missing_and_unsupported_never_read_and_io_error_is_retained() {
        let mut registers = Model::default();
        assert_eq!(snapshot(None, &mut registers),
                   Err(PinctrlError::Missing));
        assert_eq!(snapshot(state(PinctrlProvider::Unsupported),
                            &mut registers),
                   Err(PinctrlError::UnsupportedProvider));
        assert_eq!(registers.reads, 0);
        registers.fail = true;
        assert_eq!(snapshot(state(PinctrlProvider::Loongson2k { mmio:
                                                                    MmioRegion { base : 4,
                                                                                 size : 0x18 } }),
                            &mut registers),
                   Err(PinctrlError::Io));
    }
}

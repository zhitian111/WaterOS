//! Read-only Loongson-2 MMC pinmux evidence.
//!
//! Linux mainline maps SDIO to bit 20 and the PWM2/GPIO22 card-detect mux to
//! bit 14 of the first pinctrl register. Physical semantics remain
//! `UNVERIFIED_ON_HARDWARE`; this module never writes the register.

use crate::topology::{MmcPinctrlDescription, PinctrlProvider};
use core::marker::PhantomData;

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
    mux_raw : u32,
    sdio_selected : bool,
    card_detect_gpio_selected : bool,
}

impl PinctrlSnapshot {
    pub const fn mux_raw(&self) -> u32 { self.mux_raw }

    pub const fn sdio_selected(&self) -> bool { self.sdio_selected }

    pub const fn card_detect_gpio_selected(&self) -> bool { self.card_detect_gpio_selected }
}

/// Snapshot has been observed but not yet accepted as activation evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Observed;

/// Both upstream-required mux selections were observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ready;

/// At least one required mux selection was not observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NeedsTransition;

/// Typestate proof derived only from a read-only snapshot.
///
/// This proof is instantaneous and does not grant permission to write pinmux
/// registers or remove the board's `PinControlUnavailable` blocker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinctrlState<S> {
    snapshot : PinctrlSnapshot,
    state : PhantomData<S>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransitionPlan {
    pub original_raw : u32,
    pub desired_raw : u32,
    pub set_mask : u32,
    pub clear_mask : u32,
}

impl PinctrlState<Observed> {
    pub const fn new(snapshot : PinctrlSnapshot) -> Self {
        Self { snapshot,
               state : PhantomData }
    }

    pub fn classify(self) -> Result<PinctrlState<Ready>, PinctrlState<NeedsTransition>> {
        if self.snapshot
               .mux_raw &
           SDIO_BIT !=
           0 &&
           self.snapshot
               .mux_raw &
           CARD_DETECT_GPIO_BIT ==
           0
        {
            Ok(PinctrlState { snapshot : self.snapshot,
                              state : PhantomData })
        } else {
            Err(PinctrlState { snapshot : self.snapshot,
                               state : PhantomData })
        }
    }
}

impl PinctrlState<Ready> {
    pub const fn snapshot(&self) -> PinctrlSnapshot { self.snapshot }
}

impl PinctrlState<NeedsTransition> {
    pub const fn snapshot(&self) -> PinctrlSnapshot { self.snapshot }

    /// Describe the minimal upstream-derived RMW without performing it.
    pub const fn transition_plan(&self) -> TransitionPlan {
        let set_mask = if self.snapshot
                              .mux_raw &
                          SDIO_BIT !=
                          0
        {
            0
        } else {
            SDIO_BIT
        };
        let clear_mask = if self.snapshot
                                .mux_raw &
                            CARD_DETECT_GPIO_BIT ==
                            0
        {
            0
        } else {
            CARD_DETECT_GPIO_BIT
        };
        TransitionPlan { original_raw : self.snapshot
                                            .mux_raw,
                         desired_raw : (self.snapshot
                                            .mux_raw |
                                        set_mask) &
                                       !clear_mask,
                         set_mask,
                         clear_mask }
    }
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

    #[test]
    fn only_a_fully_selected_snapshot_produces_a_ready_token() {
        let ready = PinctrlState::<Observed>::new(PinctrlSnapshot {
            mux_raw : SDIO_BIT,
            sdio_selected : true,
            card_detect_gpio_selected : true,
        }).classify().expect("ready pinmux token");
        assert_eq!(ready.snapshot()
                        .mux_raw(),
                   SDIO_BIT);

        for snapshot in [PinctrlSnapshot { mux_raw : 0,
                                           sdio_selected : false,
                                           card_detect_gpio_selected : true },
                         PinctrlSnapshot { mux_raw : SDIO_BIT | CARD_DETECT_GPIO_BIT,
                                           sdio_selected : true,
                                           card_detect_gpio_selected : false }]
        {
            assert!(PinctrlState::<Observed>::new(snapshot).classify()
                                                           .is_err());
        }
    }

    #[test]
    fn transition_plan_changes_only_the_two_upstream_mux_bits() {
        let unrelated = (1 << 3) | (1 << 27);
        let snapshot = PinctrlSnapshot { mux_raw : unrelated | CARD_DETECT_GPIO_BIT,
                                         sdio_selected : false,
                                         card_detect_gpio_selected : false };
        let needed = PinctrlState::<Observed>::new(snapshot).classify()
                                                            .expect_err("transition required");
        let plan = needed.transition_plan();
        assert_eq!(plan.set_mask, SDIO_BIT);
        assert_eq!(plan.clear_mask, CARD_DETECT_GPIO_BIT);
        assert_eq!(plan.desired_raw, unrelated | SDIO_BIT);
        assert_eq!((plan.original_raw ^ plan.desired_raw) & !(SDIO_BIT | CARD_DETECT_GPIO_BIT),
                   0);
    }
}

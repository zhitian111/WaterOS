//! Read-only Loongson-2K1000 clock diagnostics and coherence evidence.
//!
//! The documented MMC parent is `LOONGSON2_APB_CLK`. Linux models its path as
//! 100 MHz reference -> DC PLL -> GMAC divider -> APB scale. This module never
//! writes a clock register and must not be treated as proof that the physical
//! clock is stable or usable (`UNVERIFIED_ON_HARDWARE`). The shared APB parent
//! is deliberately not reprogrammed: upstream exposes no MMC-private gate or
//! rate control, so changing it could disturb unrelated devices.

use crate::topology::MmcClockProvider;

const DC_PLL_OFFSET : usize = 0x20;
const GMAC_DIV_OFFSET : usize = 0x28;
const APB_SCALE_OFFSET : usize = 0x50;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockError {
    Io,
    ZeroReference,
    ZeroPllMultiplier,
    ZeroPllDivisor,
    RateOverflow,
    Inconsistent,
    UnsupportedProvider,
}

pub trait RegisterIo {
    fn read32(&mut self, offset : usize) -> Result<u32, ClockError>;
    fn read64(&mut self, offset : usize) -> Result<u64, ClockError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockSnapshot {
    pub dc_pll_raw : u64,
    pub gmac_div_raw : u32,
    pub apb_scale_raw : u64,
    pub reference_hz : u32,
    pub dc_pll_hz : u64,
    pub gmac_hz : u64,
    pub apb_hz : u64,
}

/// Two consecutive, identical software observations.
///
/// This opaque value rejects an obvious concurrent transition, but it is not
/// physical frequency/stability proof and grants no register-write authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsistentClockSnapshot {
    snapshot : ClockSnapshot,
}

impl ConsistentClockSnapshot {
    pub fn snapshot(&self) -> ClockSnapshot { self.snapshot }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsistencyStage {
    FirstRead,
    SecondRead,
    Mismatch,
}

/// Evidence retained when a two-snapshot read-only transaction is uncertain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsistencyRecovery {
    pub stage : ConsistencyStage,
    pub error : Option<ClockError>,
    pub first : Option<ClockSnapshot>,
    pub second : Option<ClockSnapshot>,
}

impl ConsistencyRecovery {
    /// Retry only the read-only coherence check; this never changes a clock.
    pub fn revalidate<R : RegisterIo>(self,
                                      registers : &mut R,
                                      reference_hz : u32)
                                      -> Result<ConsistentClockSnapshot, Self> {
        let _ = self;
        snapshot_consistent(registers, reference_hz)
    }
}

fn field(value : u64, shift : u8, width : u8) -> u64 { (value >> shift) & ((1u64 << width) - 1) }

pub fn snapshot<R : RegisterIo>(registers : &mut R,
                                reference_hz : u32)
                                -> Result<ClockSnapshot, ClockError> {
    if reference_hz == 0 {
        return Err(ClockError::ZeroReference);
    }
    let dc_pll_raw = registers.read64(DC_PLL_OFFSET)?;
    let gmac_div_raw = registers.read32(GMAC_DIV_OFFSET)?;
    let apb_scale_raw = registers.read64(APB_SCALE_OFFSET)?;

    let multiplier = field(dc_pll_raw, 32, 10);
    let pll_divisor = field(dc_pll_raw, 26, 6);
    if multiplier == 0 {
        return Err(ClockError::ZeroPllMultiplier);
    }
    if pll_divisor == 0 {
        return Err(ClockError::ZeroPllDivisor);
    }
    let dc_pll_hz = u64::from(reference_hz).checked_mul(multiplier)
                                           .ok_or(ClockError::RateOverflow)? /
                    pll_divisor;
    // Linux registers this divider as ONE_BASED | ALLOW_ZERO: zero bypasses.
    let gmac_divisor = field(u64::from(gmac_div_raw), 22, 6).max(1);
    let gmac_hz = dc_pll_hz / gmac_divisor;
    let apb_scale = field(apb_scale_raw, 20, 3) + 1;
    let apb_hz = gmac_hz.checked_mul(apb_scale)
                        .ok_or(ClockError::RateOverflow)? /
                 8;
    Ok(ClockSnapshot { dc_pll_raw,
                       gmac_div_raw,
                       apb_scale_raw,
                       reference_hz,
                       dc_pll_hz,
                       gmac_hz,
                       apb_hz })
}

pub fn snapshot_provider<R : RegisterIo>(provider : MmcClockProvider,
                                         registers : &mut R)
                                         -> Result<ClockSnapshot, ClockError> {
    match provider {
        MmcClockProvider::Loongson2k { reference_hz, .. } => snapshot(registers, reference_hz),
        MmcClockProvider::Unsupported { .. } => Err(ClockError::UnsupportedProvider),
    }
}

/// Read the complete parent chain twice and require identical generations.
///
/// This detects changes between snapshots but cannot prove that the clock did
/// not glitch between individual register reads.
pub fn snapshot_consistent<R : RegisterIo>(
    registers : &mut R,
    reference_hz : u32)
    -> Result<ConsistentClockSnapshot, ConsistencyRecovery> {
    let first = snapshot(registers, reference_hz).map_err(|error| {
                                                     ConsistencyRecovery {
                                                 stage : ConsistencyStage::FirstRead,
                                                 error : Some(error),
                                                 first : None,
                                                 second : None,
                                             }
                                                 })?;
    let second = snapshot(registers, reference_hz).map_err(|error| {
                                                      ConsistencyRecovery {
                                                  stage : ConsistencyStage::SecondRead,
                                                  error : Some(error),
                                                  first : Some(first),
                                                  second : None,
                                              }
                                                  })?;
    if first != second {
        return Err(ConsistencyRecovery { stage : ConsistencyStage::Mismatch,
                                         error : None,
                                         first : Some(first),
                                         second : Some(second) });
    }
    Ok(ConsistentClockSnapshot { snapshot : second })
}

pub fn snapshot_provider_consistent<R : RegisterIo>(
    provider : MmcClockProvider,
    registers : &mut R)
    -> Result<ConsistentClockSnapshot, ConsistencyRecovery> {
    match provider {
        MmcClockProvider::Loongson2k { reference_hz, .. } => {
            snapshot_consistent(registers, reference_hz)
        }
        MmcClockProvider::Unsupported { .. } => {
            Err(ConsistencyRecovery { stage : ConsistencyStage::FirstRead,
                                      error : Some(ClockError::UnsupportedProvider),
                                      first : None,
                                      second : None })
        }
    }
}

/// Volatile read-only backend for a topology-validated 0x58-byte window.
#[cfg(target_arch = "loongarch64")]
pub struct VolatileRegisters {
    base : *const u8,
    size : usize,
}

#[cfg(target_arch = "loongarch64")]
impl VolatileRegisters {
    /// The caller must exclusively own a valid mapped clock window for the
    /// lifetime of this backend. Construction does not access hardware.
    pub unsafe fn new(base : usize, size : usize) -> Result<Self, ClockError> {
        if base == 0 || base % 8 != 0 || size < 0x58 {
            return Err(ClockError::Io);
        }
        Ok(Self { base : base as *const u8,
                  size })
    }

    fn contains(&self, offset : usize, width : usize) -> bool {
        offset.checked_add(width)
              .is_some_and(|end| end <= self.size)
    }
}

#[cfg(target_arch = "loongarch64")]
impl RegisterIo for VolatileRegisters {
    fn read32(&mut self, offset : usize) -> Result<u32, ClockError> {
        if offset % 4 != 0 || !self.contains(offset, 4) {
            return Err(ClockError::Io);
        }
        // SAFETY: upheld by new's caller; range and alignment checked above.
        Ok(unsafe {
            core::ptr::read_volatile(self.base
                                         .add(offset)
                                         .cast::<u32>())
        })
    }

    fn read64(&mut self, offset : usize) -> Result<u64, ClockError> {
        if offset % 8 != 0 || !self.contains(offset, 8) {
            return Err(ClockError::Io);
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
    use alloc::vec::Vec;

    struct Model {
        dc_pll : u64,
        gmac_div : u32,
        apb_scale : u64,
        fail_at : Option<usize>,
        reads : Vec<(usize, usize)>,
    }

    impl RegisterIo for Model {
        fn read32(&mut self, offset : usize) -> Result<u32, ClockError> {
            self.reads
                .push((offset, 4));
            if self.fail_at == Some(offset) {
                Err(ClockError::Io)
            } else {
                Ok(self.gmac_div)
            }
        }
        fn read64(&mut self, offset : usize) -> Result<u64, ClockError> {
            self.reads
                .push((offset, 8));
            if self.fail_at == Some(offset) {
                Err(ClockError::Io)
            } else if offset == DC_PLL_OFFSET {
                Ok(self.dc_pll)
            } else {
                Ok(self.apb_scale)
            }
        }
    }

    fn model() -> Model {
        // 100 MHz * 10 / 2 = 500 MHz; /4 = 125 MHz; *4/8 = 62.5 MHz.
        Model { dc_pll : (10u64 << 32) | (2u64 << 26),
                gmac_div : 4 << 22,
                apb_scale : 3 << 20,
                fail_at : None,
                reads : Vec::new() }
    }

    #[test]
    fn snapshots_documented_apb_path_in_fixed_read_order() {
        let mut io = model();
        let state = snapshot(&mut io, 100_000_000).unwrap();
        assert_eq!(state.dc_pll_hz, 500_000_000);
        assert_eq!(state.gmac_hz, 125_000_000);
        assert_eq!(state.apb_hz, 62_500_000);
        assert_eq!(io.reads, [(0x20, 8),
                              (0x28, 4),
                              (0x50, 8)]);
    }

    #[test]
    fn accepts_linux_allow_zero_divider_as_bypass() {
        let mut io = model();
        io.gmac_div = 0;
        assert_eq!(snapshot(&mut io, 100_000_000).unwrap()
                                                 .gmac_hz,
                   500_000_000);
    }

    #[test]
    fn rejects_invalid_rates_and_propagates_each_read_failure() {
        let mut io = model();
        assert_eq!(snapshot(&mut io, 0),
                   Err(ClockError::ZeroReference));
        assert!(io.reads.is_empty());

        for offset in [DC_PLL_OFFSET,
                       GMAC_DIV_OFFSET,
                       APB_SCALE_OFFSET]
        {
            let mut io = model();
            io.fail_at = Some(offset);
            assert_eq!(snapshot(&mut io, 100_000_000),
                       Err(ClockError::Io));
        }
        let mut io = model();
        io.dc_pll = 2 << 26;
        assert_eq!(snapshot(&mut io, 100_000_000),
                   Err(ClockError::ZeroPllMultiplier));
        let mut io = model();
        io.dc_pll = 10 << 32;
        assert_eq!(snapshot(&mut io, 100_000_000),
                   Err(ClockError::ZeroPllDivisor));
    }

    #[test]
    fn topology_provider_supplies_reference_and_unsupported_never_reads() {
        let mut io = model();
        let provider = MmcClockProvider::Loongson2k { mmio : api_v0::MmioRegion { base:
                                                                                      0x1FE0_0480,
                                                                                  size : 0x58 },
                                                      reference_hz : 100_000_000 };
        assert_eq!(snapshot_provider(provider, &mut io).unwrap()
                                                       .apb_hz,
                   62_500_000);

        let mut io = model();
        assert_eq!(snapshot_provider(MmcClockProvider::Unsupported { phandle : 7 },
                                     &mut io),
                   Err(ClockError::UnsupportedProvider));
        assert!(io.reads.is_empty());
    }

    #[test]
    fn consistent_snapshot_requires_two_identical_complete_reads() {
        let mut io = model();
        let evidence = snapshot_consistent(&mut io, 100_000_000).unwrap();
        assert_eq!(evidence.snapshot()
                           .apb_hz,
                   62_500_000);
        assert_eq!(io.reads, [(0x20, 8),
                              (0x28, 4),
                              (0x50, 8),
                              (0x20, 8),
                              (0x28, 4),
                              (0x50, 8)]);
    }

    struct ChangingModel {
        inner : Model,
        gmac_reads : usize,
        second_gmac : u32,
        fail_call : Option<usize>,
        calls : usize,
    }

    impl RegisterIo for ChangingModel {
        fn read32(&mut self, offset : usize) -> Result<u32, ClockError> {
            self.calls += 1;
            if self.fail_call == Some(self.calls) {
                return Err(ClockError::Io);
            }
            assert_eq!(offset, GMAC_DIV_OFFSET);
            self.gmac_reads += 1;
            Ok(if self.gmac_reads == 1 {
                self.inner.gmac_div
            } else {
                self.second_gmac
            })
        }

        fn read64(&mut self, offset : usize) -> Result<u64, ClockError> {
            self.calls += 1;
            if self.fail_call == Some(self.calls) {
                return Err(ClockError::Io);
            }
            match offset {
                DC_PLL_OFFSET => Ok(self.inner.dc_pll),
                APB_SCALE_OFFSET => Ok(self.inner.apb_scale),
                _ => panic!("unexpected clock offset {offset:#x}"),
            }
        }
    }

    fn changing(second_gmac : u32, fail_call : Option<usize>) -> ChangingModel {
        ChangingModel { inner : model(),
                        gmac_reads : 0,
                        second_gmac,
                        fail_call,
                        calls : 0 }
    }

    #[test]
    fn mismatch_retains_both_generations_and_can_revalidate() {
        let mut io = changing(5 << 22, None);
        let recovery = snapshot_consistent(&mut io, 100_000_000).unwrap_err();
        assert_eq!(recovery.stage,
                   ConsistencyStage::Mismatch);
        assert_eq!(recovery.first
                           .unwrap()
                           .gmac_div_raw,
                   4 << 22);
        assert_eq!(recovery.second
                           .unwrap()
                           .gmac_div_raw,
                   5 << 22);

        let mut stable = model();
        assert_eq!(recovery.revalidate(&mut stable, 100_000_000)
                           .unwrap()
                           .snapshot()
                           .apb_hz,
                   62_500_000);
    }

    #[test]
    fn read_failure_reports_stage_without_inventing_later_evidence() {
        let mut first = changing(4 << 22, Some(1));
        let failure = snapshot_consistent(&mut first, 100_000_000).unwrap_err();
        assert_eq!(failure.stage,
                   ConsistencyStage::FirstRead);
        assert_eq!(failure.error, Some(ClockError::Io));
        assert_eq!(failure.first, None);

        let mut second = changing(4 << 22, Some(5));
        let failure = snapshot_consistent(&mut second, 100_000_000).unwrap_err();
        assert_eq!(failure.stage,
                   ConsistencyStage::SecondRead);
        assert_eq!(failure.error, Some(ClockError::Io));
        assert!(failure.first
                       .is_some());
        assert_eq!(failure.second, None);
    }
}

//! Minimal JH7110 SYSCRG support used by the VisionFive 2 MMC path.
//!
//! This is intentionally narrower than a generic clock/reset framework: it
//! ports the TGOSKits SDIO clock/reset register handling needed by JH7110 MMC
//! hosts and keeps the board-specific resource binding in this crate.

use api_v0::MmioRegion;

use crate::mmc::{MmcHostDescription, ResourceSpecifier};

const JH7110_SYSCLK_SDIO0_AHB : u32 = 91;
const JH7110_SYSCLK_SDIO1_AHB : u32 = 92;
const JH7110_SYSCLK_SDIO0_SDCARD : u32 = 93;
const JH7110_SYSCLK_SDIO1_SDCARD : u32 = 94;

const JH7110_SYSRST_SDIO0_AHB : u32 = 64;
const JH7110_SYSRST_SDIO1_AHB : u32 = 65;

const CLOCK_CONTROL_ENABLE : u32 = 1 << 31;
const CLOCK_CONTROL_DIV_MASK : u32 = (1 << 24) - 1;
const SDIO_SDCARD_PARENT_HZ : u64 = 400_000_000;
const SDIO_SDCARD_MAX_DIV : u32 = 15;
const RESET_ASSERT_OFFSET : usize = 0x2F8;
const RESET_STATUS_OFFSET : usize = 0x308;
const RESET_STATUS_POLL_LIMIT : usize = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyscrgError {
    MissingProviderMmio,
    MmioTooSmall,
    MissingSpecifierArg,
    UnsupportedClock(u32),
    UnsupportedReset(u32),
    InvalidRate,
    ResetPollTimeout(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MmcHostPreparation {
    pub biu_clock_id : u32,
    pub ciu_clock_id : u32,
    pub reset_id : u32,
    pub ciu_rate_hz : u64,
}

pub fn prepare_mmc_host(host : &MmcHostDescription,
                        ciu_rate_hz : u64)
                        -> Result<MmcHostPreparation, SyscrgError> {
    let biu_clock_id = enable_clock(&host.biu_clock)?;
    let ciu_rate_hz = set_sdcard_rate(&host.ciu_clock, ciu_rate_hz)?;
    let ciu_clock_id = enable_clock(&host.ciu_clock)?;
    let reset_id = deassert_reset(&host.reset)?;
    if let Some(sysreg) = host.sysreg {
        log::info!("[driver][visionfive2] MMC sysreg provider={} mmio={:?} offset={:#x} shift={} \
                    mask={:#x}; no TGOSKits write sequence applied",
                   sysreg.provider,
                   sysreg.provider_mmio,
                   sysreg.offset,
                   sysreg.shift,
                   sysreg.mask);
    }
    Ok(MmcHostPreparation { biu_clock_id,
                            ciu_clock_id,
                            reset_id,
                            ciu_rate_hz })
}

pub fn test() {
    log::info!("[driver][visionfive2] syscrg SDIO clocks=({}, {}, {}, {}) resets=({}, {}); \
                hardware MMIO status=UNVERIFIED",
               JH7110_SYSCLK_SDIO0_AHB,
               JH7110_SYSCLK_SDIO1_AHB,
               JH7110_SYSCLK_SDIO0_SDCARD,
               JH7110_SYSCLK_SDIO1_SDCARD,
               JH7110_SYSRST_SDIO0_AHB,
               JH7110_SYSRST_SDIO1_AHB);
}

fn specifier_id(specifier : &ResourceSpecifier) -> Result<u32, SyscrgError> {
    specifier.args
             .first()
             .copied()
             .ok_or(SyscrgError::MissingSpecifierArg)
}

fn provider_mmio(specifier : &ResourceSpecifier,
                 min_size : usize)
                 -> Result<MmioRegion, SyscrgError> {
    let mmio = specifier.provider_mmio
                        .ok_or(SyscrgError::MissingProviderMmio)?;
    let end = mmio.base
                  .checked_add(mmio.size)
                  .ok_or(SyscrgError::MmioTooSmall)?;
    if mmio.base == 0 || mmio.size < min_size || end <= mmio.base {
        return Err(SyscrgError::MmioTooSmall);
    }
    Ok(mmio)
}

fn supported_sdio_clock(id : u32) -> bool {
    matches!(id,
             JH7110_SYSCLK_SDIO0_AHB |
             JH7110_SYSCLK_SDIO1_AHB |
             JH7110_SYSCLK_SDIO0_SDCARD |
             JH7110_SYSCLK_SDIO1_SDCARD)
}

fn clock_control_offset(id : u32) -> Result<usize, SyscrgError> {
    if !supported_sdio_clock(id) {
        return Err(SyscrgError::UnsupportedClock(id));
    }
    usize::try_from(id).ok()
                       .and_then(|id| id.checked_mul(core::mem::size_of::<u32>()))
                       .ok_or(SyscrgError::MmioTooSmall)
}

fn reset_word_bit(id : u32) -> Result<(usize, u32), SyscrgError> {
    if !matches!(id,
                 JH7110_SYSRST_SDIO0_AHB | JH7110_SYSRST_SDIO1_AHB)
    {
        return Err(SyscrgError::UnsupportedReset(id));
    }
    Ok(((id / u32::BITS) as usize, 1_u32 << (id % u32::BITS)))
}

fn enable_clock(specifier : &ResourceSpecifier) -> Result<u32, SyscrgError> {
    let id = specifier_id(specifier)?;
    let offset = clock_control_offset(id)?;
    let mmio = provider_mmio(specifier,
                             offset + core::mem::size_of::<u32>())?;
    let ptr = (mmio.base + offset) as *mut u32;
    // SAFETY: DTB supplied this MMIO window for the JH7110 syscrg provider; the
    // board driver serializes early bring-up and only touches the SDIO fields.
    unsafe {
        let value = core::ptr::read_volatile(ptr);
        core::ptr::write_volatile(ptr, value | CLOCK_CONTROL_ENABLE);
    }
    Ok(id)
}

fn set_sdcard_rate(specifier : &ResourceSpecifier, rate : u64) -> Result<u64, SyscrgError> {
    let id = specifier_id(specifier)?;
    if !matches!(id,
                 JH7110_SYSCLK_SDIO0_SDCARD | JH7110_SYSCLK_SDIO1_SDCARD)
    {
        return Err(SyscrgError::UnsupportedClock(id));
    }
    if rate == 0 {
        return Err(SyscrgError::InvalidRate);
    }
    let offset = clock_control_offset(id)?;
    let mmio = provider_mmio(specifier,
                             offset + core::mem::size_of::<u32>())?;
    let div = divider_for_rate(rate);
    let ptr = (mmio.base + offset) as *mut u32;
    // SAFETY: see `enable_clock`; this preserves all non-DIV fields.
    unsafe {
        let value = core::ptr::read_volatile(ptr);
        core::ptr::write_volatile(ptr,
                                  (value & !CLOCK_CONTROL_DIV_MASK) | div);
    }
    Ok(SDIO_SDCARD_PARENT_HZ / u64::from(div))
}

fn deassert_reset(specifier : &ResourceSpecifier) -> Result<u32, SyscrgError> {
    let id = specifier_id(specifier)?;
    let (word, mask) = reset_word_bit(id)?;
    let assert_offset = RESET_ASSERT_OFFSET + word * core::mem::size_of::<u32>();
    let status_offset = RESET_STATUS_OFFSET + word * core::mem::size_of::<u32>();
    let mmio = provider_mmio(specifier,
                             status_offset + core::mem::size_of::<u32>())?;
    let assert_ptr = (mmio.base + assert_offset) as *mut u32;
    let status_ptr = (mmio.base + status_offset) as *const u32;
    // SAFETY: see `enable_clock`; reset register layout follows TGOSKits'
    // JH7110 syscrg implementation.
    unsafe {
        let value = core::ptr::read_volatile(assert_ptr);
        core::ptr::write_volatile(assert_ptr, value & !mask);
        for _ in 0..RESET_STATUS_POLL_LIMIT {
            if core::ptr::read_volatile(status_ptr) & mask != 0 {
                return Ok(id);
            }
            core::hint::spin_loop();
        }
    }
    Err(SyscrgError::ResetPollTimeout(id))
}

fn divider_for_rate(rate : u64) -> u32 {
    let rounded = (SDIO_SDCARD_PARENT_HZ + rate / 2) / rate;
    u32::try_from(rounded).unwrap_or(SDIO_SDCARD_MAX_DIV)
                          .clamp(1, SDIO_SDCARD_MAX_DIV)
}

#[cfg(test)]
mod tests {
    use alloc::vec;

    use super::*;

    fn fake_syscrg() -> (alloc::vec::Vec<u32>, MmioRegion) {
        let mut regs = vec![0_u32; 0x10000 / core::mem::size_of::<u32>()];
        let base = regs.as_mut_ptr() as usize;
        (regs,
         MmioRegion { base,
                      size : 0x10000 })
    }

    fn spec(mmio : MmioRegion, id : u32) -> ResourceSpecifier {
        ResourceSpecifier { provider : 1,
                            provider_mmio : Some(mmio),
                            args : vec![id] }
    }

    #[test]
    fn enables_sdio_clock_and_sets_rate_divider() {
        let (regs, mmio) = fake_syscrg();
        let clock = spec(mmio, JH7110_SYSCLK_SDIO1_SDCARD);
        assert_eq!(set_sdcard_rate(&clock, 50_000_000),
                   Ok(50_000_000));
        assert_eq!(enable_clock(&clock),
                   Ok(JH7110_SYSCLK_SDIO1_SDCARD));

        let value = regs[JH7110_SYSCLK_SDIO1_SDCARD as usize];
        assert_eq!(value & CLOCK_CONTROL_DIV_MASK, 8);
        assert_ne!(value & CLOCK_CONTROL_ENABLE, 0);
        drop(regs);
    }

    #[test]
    fn deasserts_sdio_reset_and_polls_status() {
        let (mut regs, mmio) = fake_syscrg();
        let word = (JH7110_SYSRST_SDIO1_AHB / u32::BITS) as usize;
        let bit = 1_u32 << (JH7110_SYSRST_SDIO1_AHB % u32::BITS);
        let assert_index = RESET_ASSERT_OFFSET / core::mem::size_of::<u32>() + word;
        let status_index = RESET_STATUS_OFFSET / core::mem::size_of::<u32>() + word;
        regs[assert_index] = bit;
        regs[status_index] = bit;

        assert_eq!(deassert_reset(&spec(mmio, JH7110_SYSRST_SDIO1_AHB)),
                   Ok(JH7110_SYSRST_SDIO1_AHB));
        assert_eq!(regs[assert_index] & bit, 0);
        drop(regs);
    }

    #[test]
    fn rejects_unsupported_ids_and_missing_mmio() {
        let (_, mmio) = fake_syscrg();
        assert_eq!(enable_clock(&spec(mmio, 1)),
                   Err(SyscrgError::UnsupportedClock(1)));
        assert_eq!(deassert_reset(&spec(mmio, 1)),
                   Err(SyscrgError::UnsupportedReset(1)));
        assert_eq!(enable_clock(&ResourceSpecifier { provider : 1,
                                                     provider_mmio : None,
                                                     args : vec![JH7110_SYSCLK_SDIO0_AHB] }),
                   Err(SyscrgError::MissingProviderMmio));
    }
}

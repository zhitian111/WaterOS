#![no_std]
#[cfg(feature = "api-v0")]
pub mod boot {
    pub use api_v0::boot::{PlatformBootArgs, PlatformBootContext};
    #[cfg(feature = "impl-dummy")]
    pub use impl_dummy::boot::PlatformDummyBootArgs as BootArgs;
    #[cfg(feature = "impl-dummy")]
    pub use impl_dummy::boot::PlatformDummyBootContext as BootContext;
    #[cfg(feature = "impl-qemu-riscv64-opensbi")]
    pub use impl_qemu_riscv64_opensbi::boot::QEMURiscv64OpenSBIBootArgs as BootArgs;
    #[cfg(feature = "impl-qemu-riscv64-opensbi")]
    pub use impl_qemu_riscv64_opensbi::boot::QEMURiscv64OpenSBIBootContext as BootContext;
}

pub mod arch {
    pub fn init() { arch::arch_boot(); }
}

#[cfg(feature = "api-v0")]
pub mod time {
    pub use api_v0::time::{PlatformTime, PlatformTimeError, PlatformTimeResult};

    #[cfg(feature = "impl-dummy")]
    pub use impl_dummy::time::PlatformDummyTime as PlatformTimeImpl;
    #[cfg(feature = "impl-qemu-riscv64-opensbi")]
    pub use impl_qemu_riscv64_opensbi::time::QEMURiscv64OpenSBITime as PlatformTimeImpl;

    #[inline]
    pub fn frequency_hz() -> PlatformTimeResult<u64> { PlatformTimeImpl::time_frequency_hz() }
}

pub mod timer {
    use core::time::Duration;

    pub use api_v0::time::PlatformTimeError;
    pub use arch::time::{ArchTimeError, ArchTimeFrequency, ArchTimeTick};
    pub use firmware::timer::{FirmwareTimerDeadline, FirmwareTimerError};

    #[derive(Debug)]
    pub enum PlatformTimerError {
        Arch(ArchTimeError),
        Platform(PlatformTimeError),
        Firmware(FirmwareTimerError),
        NoFrequency,
        Overflow,
    }

    pub type PlatformTimerResult<T> = core::result::Result<T, PlatformTimerError>;

    #[inline]
    pub fn now_tick() -> PlatformTimerResult<ArchTimeTick> {
        arch::time::read_time_tick().map_err(PlatformTimerError::Arch)
    }

    #[inline]
    pub fn tick_hz() -> PlatformTimerResult<ArchTimeFrequency> {
        let hz = crate::time::frequency_hz().map_err(PlatformTimerError::Platform)?;
        Ok(ArchTimeFrequency(hz))
    }

    #[inline]
    pub fn set_timer_deadline_tick(deadline_tick : ArchTimeTick) -> PlatformTimerResult<()> {
        firmware::timer::set_timer(FirmwareTimerDeadline(deadline_tick.0))
            .map_err(PlatformTimerError::Firmware)
    }

    #[inline]
    fn duration_to_ticks(d : Duration, hz : u64) -> PlatformTimerResult<u64> {
        // 向上取整，避免过早触发（例如 1ns 在低频下被截断成 0 tick）。
        let nanos = d.as_nanos();
        let ticks = nanos.checked_mul(hz as u128)
                         .ok_or(PlatformTimerError::Overflow)?
                         .checked_add(1_000_000_000u128 - 1)
                         .ok_or(PlatformTimerError::Overflow)? /
                    1_000_000_000u128;
        u64::try_from(ticks).map_err(|_| PlatformTimerError::Overflow)
    }

    #[inline]
    fn ticks_to_duration(ticks : u64, hz : u64) -> PlatformTimerResult<Duration> {
        if hz == 0 {
            return Err(PlatformTimerError::NoFrequency);
        }
        let nanos = (ticks as u128).checked_mul(1_000_000_000u128)
                                   .ok_or(PlatformTimerError::Overflow)? /
                    (hz as u128);
        let nanos_u64 = u64::try_from(nanos).map_err(|_| PlatformTimerError::Overflow)?;
        Ok(Duration::from_nanos(nanos_u64))
    }

    #[inline]
    pub fn now_duration() -> PlatformTimerResult<Duration> {
        let tick = now_tick()?.0;
        let hz = tick_hz()?.0;
        ticks_to_duration(tick, hz)
    }

    #[inline]
    pub fn set_timer_after(d : Duration) -> PlatformTimerResult<()> {
        let now = now_tick()?.0;
        let hz = tick_hz()?.0;
        if hz == 0 {
            return Err(PlatformTimerError::NoFrequency);
        }
        let delta = duration_to_ticks(d, hz)?;
        let deadline = now.checked_add(delta)
                          .ok_or(PlatformTimerError::Overflow)?;
        logging::debug!("now is :{:?}, and will set to :{:?}",
                        now,
                        deadline);
        set_timer_deadline_tick(ArchTimeTick(deadline))
    }

    #[inline]
    pub fn set_timer_after_ms(ms : u64) -> PlatformTimerResult<()> {
        set_timer_after(Duration::from_millis(ms))
    }

    #[inline]
    pub fn set_timer_after_s(s : u64) -> PlatformTimerResult<()> {
        set_timer_after(Duration::from_secs(s))
    }
}
pub mod reset {
    pub use firmware::reset::*;
}
pub mod console {
    pub use firmware::console::*;
}

pub mod interrupt {
    pub use arch::interrupt::*;
}

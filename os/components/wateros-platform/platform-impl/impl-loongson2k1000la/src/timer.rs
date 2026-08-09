use api_v0::timer::{
    PlatformDeadlineTimerError, PlatformDeadlineTimerResult, PlatformTimerDeadline,
};

const CSR_TCFG : usize = 0x41;
const CSR_TICLR : usize = 0x44;

pub fn set_timer(time : PlatformTimerDeadline) -> PlatformDeadlineTimerResult<()> {
    let now : u64;
    let counter_id : usize;
    unsafe {
        core::arch::asm!("rdtime.d {0}, {1}", out(reg) now, out(reg) counter_id);
    }
    let _ = counter_id;
    let delta =
        usize::try_from(time.0
                            .saturating_sub(now)
                            .max(1)).map_err(|_| PlatformDeadlineTimerError::InvalidDeadline)?;
    let ticks = delta.checked_add(3)
                     .ok_or(PlatformDeadlineTimerError::InvalidDeadline)? &
                !3;
    unsafe {
        core::arch::asm!("csrwr {0}, {1}", inout(reg) 1usize => _, const CSR_TICLR);
        core::arch::asm!("csrwr {0}, {1}", inout(reg) (ticks | 1) => _, const CSR_TCFG);
    }
    Ok(())
}

//! QEMU LoongArch64 `virt` CSR 定时器 deadline 编程。
//!
//! StableCounter tick 来自 `rdtime.d`；deadline 与 `platform::timer` 传入的 tick
//! 使用同一刻度。

use core::arch::asm;

use api_v0::timer::{
    PlatformDeadlineTimerError, PlatformDeadlineTimerResult, PlatformTimerDeadline,
};

/// 定时器配置 CSR：写入 `(delta << 2) | ENABLE` 形式与硬件解码约定一致。
const CSR_TCFG: usize = 0x41;
/// 定时器中断清除 CSR。
const CSR_TICLR: usize = 0x44;
const TCFG_ENABLE: usize = 1 << 0;
const TICLR_CLEAR_TIMER: usize = 1 << 0;

#[inline]
fn read_stable_counter() -> u64 {
    let tick: u64;
    let _counter_id: usize;
    unsafe {
        asm!("rdtime.d {0}, {1}", out(reg) tick, out(reg) _counter_id);
    }
    tick
}

#[inline]
fn write_csr<const CSR: usize>(value: usize) {
    let old = value;
    unsafe {
        asm!("csrwr {0}, {1}", inout(reg) old => _, const CSR);
    }
}

/// 设置下一次定时器中断 deadline。
#[inline]
pub fn set_timer(time: PlatformTimerDeadline) -> PlatformDeadlineTimerResult<()> {
    let now = read_stable_counter();
    let delta = time
        .0
        .saturating_sub(now)
        .max(1);
    let delta = usize::try_from(delta)
        .map_err(|_| PlatformDeadlineTimerError::InvalidDeadline)?;
    if delta > (usize::MAX >> 2) {
        return Err(PlatformDeadlineTimerError::InvalidDeadline);
    }
    write_csr::<CSR_TICLR>(TICLR_CLEAR_TIMER);
    write_csr::<CSR_TCFG>((delta << 2) | TCFG_ENABLE);
    Ok(())
}

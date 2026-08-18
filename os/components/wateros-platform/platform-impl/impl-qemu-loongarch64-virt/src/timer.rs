//! QEMU LoongArch64 `virt` CSR 定时器 deadline 编程。
//!
//! StableCounter tick 来自 `rdtime.d`；deadline 与 `platform::timer` 传入的 tick
//! 使用同一刻度。

use core::arch::asm;

use api_v0::timer::{
    PlatformDeadlineTimerError, PlatformDeadlineTimerResult, PlatformTimerDeadline,
};

/// 定时器配置 CSR：低两位为控制位，计时值必须按 4 tick 对齐。
const CSR_TCFG: usize = 0x41;
/// 定时器中断清除 CSR。
const CSR_TICLR: usize = 0x44;
/// TCFG 使能位。
const TCFG_ENABLE: usize = 1 << 0;
/// TICLR 定时器清除位。
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
/// 写定时器相关 CSR；仅允许本文件的 `CSR_TCFG` / `CSR_TICLR` 常量作为参数。
fn write_csr<const CSR: usize>(value: usize) {
    let old = value;
    unsafe {
        asm!("csrwr {0}, {1}", inout(reg) old => _, const CSR);
    }
}

/// 设置下一次定时器中断 deadline。
///
/// TIME_CONTRACT: `time` 为绝对 StableCounter tick；硬件 TCFG 接收的是相对 delta，
/// 因此必须在本 CPU 上紧邻 `rdtime.d` 读取后换算。`max(1)` 防止 past deadline
/// 关闭定时器或产生 0 tick 间隔。
#[inline]
pub fn set_timer(time: PlatformTimerDeadline) -> PlatformDeadlineTimerResult<()> {
    let now = read_stable_counter();
    let delta = time
        .0
        .saturating_sub(now)
        .max(1);
    let delta = usize::try_from(delta).map_err(|_| PlatformDeadlineTimerError::InvalidDeadline)?;
    let timer_ticks = delta
        .checked_add(3)
        .ok_or(PlatformDeadlineTimerError::InvalidDeadline)?
        & !0b11;
    write_csr::<CSR_TICLR>(TICLR_CLEAR_TIMER);
    write_csr::<CSR_TCFG>(timer_ticks | TCFG_ENABLE);
    Ok(())
}

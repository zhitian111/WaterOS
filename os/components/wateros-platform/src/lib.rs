#![no_std]

//! WaterOS **平台聚合**：把 `arch`（指令集 / CSR 原语）与 `platform-impl`
//!（板级或 QEMU 等具体环境）组合成内核可直接调用的入口。
//!
//! ## 分层与边界
//! - **`arch`（`wateros-platform-arch`）**：与 ISA 强相关的最小原语（trap 上下文、
//!   `time` CSR、中断屏蔽位、`satp` 等）。
//! - **`platform-impl`**：当前板级 profile，负责 boot 参数、时间频率、early console、
//!   deadline timer、reset 等平台能力。后端可以是 OpenSBI、MMIO UART 或其它机制。
//! - **本 crate**：在以上两层之上提供**组合语义**（例如 [`timer`] 将 arch 的 tick
//!   与平台 deadline timer 编程衔接），并由 `platform-api-v0` 定义跨实现的 trait
//!   契约。
//!
//! 默认 feature 与成员 crate 对应关系以根目录 `Cargo.toml` 为准；文档描述的是各层
//! **语义契约**与替换点，而非单一硬件路径的实现细节。

/// 当前 feature 选中的板级实现 crate（`impl-dummy` / QEMU profile 之一）。
#[cfg(feature = "impl-dummy")]
pub use impl_dummy as active_impl;
/// 当前 feature 选中的板级实现 crate（LoongArch QEMU virt）。
#[cfg(feature = "impl-qemu-loongarch64-virt")]
pub use impl_qemu_loongarch64_virt as active_impl;
/// 当前 feature 选中的板级实现 crate（RISC-V QEMU + OpenSBI）。
#[cfg(feature = "impl-qemu-riscv64-opensbi")]
pub use impl_qemu_riscv64_opensbi as active_impl;

/// 启动参数与引导上下文：具体类型由 feature 选中的 `platform-impl` 提供。
#[cfg(feature = "api-v0")]
pub mod boot {
    pub use api_v0::boot::{PlatformBootArgs, PlatformBootContext};
    pub use crate::active_impl::boot::{BootArgs, BootContext};
}

/// 架构层再导出：trap、时间计数、中断控制、分页等与 ISA 直接相关的 API。
///
/// 与 **platform-impl** 的边界：仅操作 CPU / 监管态可见的硬件与 CSR，不选择板级后端。
pub mod arch {
    pub use arch::*;

    pub fn init() {
        arch_boot();
    }
}

/// 平台层时间频率：由 `PlatformTime` 实现（通常来自板级 / DTB / 环境常量），
/// **不**等同于 arch 的 `time` CSR 读频率（arch 侧可能返回不支持）。
///
/// 引导期可通过 [`set_frequency_hz`] 注入 DTB 探测结果；未设置时回退
/// `PlatformTimeImpl::time_frequency_hz`。
#[cfg(feature = "api-v0")]
pub mod time {
    use core::sync::atomic::{AtomicU64, Ordering};

    pub use api_v0::time::{PlatformTime, PlatformTimeError, PlatformTimeResult};

    pub use crate::active_impl::time::PlatformTimeImpl;

    static TIMEBASE_HZ_CACHE: AtomicU64 = AtomicU64::new(0);

    /// 由引导代码在首次使用 [`frequency_hz`] 前写入 DTB 等探测到的 tick 频率（Hz）。
    #[inline]
    pub fn set_frequency_hz(hz: u64) -> PlatformTimeResult<()> {
        if hz == 0 {
            return Err(PlatformTimeError::InvalidFrequency);
        }
        TIMEBASE_HZ_CACHE.store(hz, Ordering::Release);
        Ok(())
    }

    #[inline]
    pub fn frequency_hz() -> PlatformTimeResult<u64> {
        let cached = TIMEBASE_HZ_CACHE.load(Ordering::Acquire);
        if cached != 0 {
            return Ok(cached);
        }
        PlatformTimeImpl::time_frequency_hz()
    }
}

/// 组合定时器：用 arch 的 tick 与 [`crate::time`] 给出的 Hz 换算时刻，再经
/// 平台后端编程下一次定时器中断。
///
/// 错误变体区分 `Arch` / `Platform` / `DeadlineTimer` 三层来源，便于定位是 CSR、
/// 频率配置还是平台定时器后端调用失败。
pub mod timer {
    use core::time::Duration;

    pub use api_v0::time::PlatformTimeError;
    pub use arch::time::{ArchTimeError, ArchTimeFrequency, ArchTimeTick};
    pub use api_v0::timer::{
        PlatformDeadlineTimerError, PlatformDeadlineTimerResult, PlatformTimerDeadline,
    };

    /// 组合定时器路径上各层失败的归并类型。
    #[derive(Debug)]
    pub enum PlatformTimerError {
        Arch(ArchTimeError),
        Platform(PlatformTimeError),
        DeadlineTimer(PlatformDeadlineTimerError),
        NoFrequency,
        Overflow,
    }

    /// [`PlatformTimerError`] 上的 `Result` 别名。
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
    pub fn set_timer_deadline_tick(deadline_tick: ArchTimeTick) -> PlatformTimerResult<()> {
        crate::active_impl::timer::set_timer(PlatformTimerDeadline(deadline_tick.0))
            .map_err(PlatformTimerError::DeadlineTimer)
    }

    #[inline]
    fn duration_to_ticks(d: Duration, hz: u64) -> PlatformTimerResult<u64> {
        // 向上取整，避免过早触发（例如 1ns 在低频下被截断成 0 tick）。
        let nanos = d.as_nanos();
        let ticks = nanos
            .checked_mul(hz as u128)
            .ok_or(PlatformTimerError::Overflow)?
            .checked_add(1_000_000_000u128 - 1)
            .ok_or(PlatformTimerError::Overflow)?
            / 1_000_000_000u128;
        u64::try_from(ticks).map_err(|_| PlatformTimerError::Overflow)
    }

    #[inline]
    fn ticks_to_duration(ticks: u64, hz: u64) -> PlatformTimerResult<Duration> {
        if hz == 0 {
            return Err(PlatformTimerError::NoFrequency);
        }
        let nanos = (ticks as u128)
            .checked_mul(1_000_000_000u128)
            .ok_or(PlatformTimerError::Overflow)?
            / (hz as u128);
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
    pub fn set_timer_after(d: Duration) -> PlatformTimerResult<()> {
        let now = now_tick()?.0;
        let hz = tick_hz()?.0;
        if hz == 0 {
            return Err(PlatformTimerError::NoFrequency);
        }
        let delta = duration_to_ticks(d, hz)?;
        let deadline = now
            .checked_add(delta)
            .ok_or(PlatformTimerError::Overflow)?;
        log::debug!(
            "now is :{:?}, and will set to :{:?}",
            now,
            deadline
        );
        set_timer_deadline_tick(ArchTimeTick(deadline))
    }

    #[inline]
    pub fn set_timer_after_ms(ms: u64) -> PlatformTimerResult<()> {
        set_timer_after(Duration::from_millis(ms))
    }

    #[inline]
    pub fn set_timer_after_s(s: u64) -> PlatformTimerResult<()> {
        set_timer_after(Duration::from_secs(s))
    }
}

/// 系统复位与关机：由当前 platform impl 提供，不经由 arch 特权指令封装。
pub mod reset {
    pub use api_v0::reset::{
        PlatformResetError, PlatformResetReason, PlatformResetResult, PlatformResetType,
    };
    pub use crate::active_impl::reset::{reboot, reset, shutdown};
}

/// 早期控制台输出：由当前 board feature 选择 OpenSBI、UART 或其它平台后端。
pub mod console {
    pub use api_v0::console::{PlatformConsoleError, PlatformConsoleResult};
    pub use crate::active_impl::console::{
        console_flush, console_write_a_buffer, console_write_a_byte,
    };
}

/// 墙上时钟（`CLOCK_REALTIME` 偏移 + 单调时钟）。
pub mod wall_clock;

/// 中断控制原语再导出：属于 **arch** 层（如 `sie` / `sstatus`），非 platform impl。
pub mod interrupt {
    pub use arch::interrupt::*;
}

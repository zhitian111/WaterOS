#![no_std]

//! WaterOS **平台聚合**：把 `arch`（指令集 / CSR 原语）、`firmware`（固件 / SBI 能力）与
//! `platform-impl`（板级或 QEMU 等具体环境）组合成内核可直接调用的入口。
//!
//! ## 分层与边界
//! - **`arch`（`wateros-platform-arch`）**：与 ISA 强相关的最小原语（trap 上下文、
//!   `time` CSR、中断屏蔽位、`satp` 等）。**不**承担“经 SBI 写下次定时器”“串口经固件输出”
//!   等固件契约——这些属于 `firmware`。
//! - **`firmware`（`wateros-platform-firmware`）**：固件 ABI 封装（控制台、
//!   `set_timer`、系统复位等）。实现随 feature 切换（如 OpenSBI、dummy）。
//! - **本 crate**：在以上两层之上提供**组合语义**（例如 [`timer`] 将 arch 的 tick
//!   与 firmware 的 deadline 编程衔接），并由 `platform-api-v0` 定义跨实现的 trait
//!   契约；**板级时间频率**等可来自 DTB/常量/固件，由选中的 `platform-impl` 决定。
//!
//! 默认 feature 与成员 crate 对应关系以根目录 `Cargo.toml` 为准；文档描述的是各层
//! **语义契约**与替换点，而非单一硬件路径的实现细节。

/// 启动参数与引导上下文：具体类型由 feature 选中的 `platform-impl` 提供。
#[cfg(feature = "api-v0")]
pub mod boot {
    pub use api_v0::boot::{PlatformBootArgs, PlatformBootContext};
    #[cfg(feature = "impl-dummy")]
    pub use impl_dummy::boot::PlatformDummyBootArgs as BootArgs;
    #[cfg(feature = "impl-dummy")]
    pub use impl_dummy::boot::PlatformDummyBootContext as BootContext;
    #[cfg(feature = "impl-qemu-loongarch64-virt")]
    pub use impl_qemu_loongarch64_virt::boot::QEMULoongArch64VirtBootArgs as BootArgs;
    #[cfg(feature = "impl-qemu-loongarch64-virt")]
    pub use impl_qemu_loongarch64_virt::boot::QEMULoongArch64VirtBootContext as BootContext;
    #[cfg(feature = "impl-qemu-riscv64-opensbi")]
    pub use impl_qemu_riscv64_opensbi::boot::QEMURiscv64OpenSBIBootArgs as BootArgs;
    #[cfg(feature = "impl-qemu-riscv64-opensbi")]
    pub use impl_qemu_riscv64_opensbi::boot::QEMURiscv64OpenSBIBootContext as BootContext;
}

/// 架构层再导出：trap、时间计数、中断控制、分页等与 ISA 直接相关的 API。
///
/// 与 **firmware** 子系统（`wateros-platform-firmware` 依赖）的边界：仅操作 CPU /
/// 监管态可见的硬件与 CSR，不调用 SBI。
pub mod arch {
    pub use ::arch::*;

    #[inline]
    pub fn init() {
        arch_boot();
    }
}

/// 平台层时间频率：由 `PlatformTime` 实现（通常来自板级 / DTB / 环境常量），
/// **不**等同于 arch 的 `time` CSR 读频率（arch 侧可能返回不支持）。
#[cfg(feature = "api-v0")]
pub mod time {
    pub use api_v0::time::{PlatformTime, PlatformTimeError, PlatformTimeResult};

    #[cfg(feature = "impl-dummy")]
    pub use impl_dummy::time::PlatformDummyTime as PlatformTimeImpl;
    #[cfg(feature = "impl-qemu-loongarch64-virt")]
    pub use impl_qemu_loongarch64_virt::time::QEMULoongArch64VirtTime as PlatformTimeImpl;
    #[cfg(feature = "impl-qemu-riscv64-opensbi")]
    pub use impl_qemu_riscv64_opensbi::time::QEMURiscv64OpenSBITime as PlatformTimeImpl;

    #[inline]
    pub fn frequency_hz() -> PlatformTimeResult<u64> { PlatformTimeImpl::time_frequency_hz() }
}

/// 组合定时器：用 arch 的 tick 与 [`crate::time`] 给出的 Hz 换算时刻，再经
/// **firmware** 子系统编程固件定时器。
///
/// 错误变体区分 `Arch` / `Platform` / `Firmware` 三层来源，便于定位是 CSR、
/// 频率配置还是 SBI 调用失败。
pub mod timer {
    use core::time::Duration;

    pub use api_v0::time::PlatformTimeError;
    pub use arch::time::{ArchTimeError, ArchTimeFrequency, ArchTimeTick};
    pub use firmware::timer::{FirmwareTimerDeadline, FirmwareTimerError};

    /// 组合定时器路径上各层失败的归并类型。
    #[derive(Debug)]
    pub enum PlatformTimerError {
        Arch(ArchTimeError),
        Platform(PlatformTimeError),
        Firmware(FirmwareTimerError),
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

/// 系统复位与关机：纯固件 / SBI 语义，不经由 arch 特权指令封装（与 `arch` 解耦）。
pub mod reset {
    pub use firmware::reset::*;
}

/// 早期控制台输出：走固件或 SBI，不直接操作 UART MMIO（与裸机驱动层解耦）。
pub mod console {
    pub use firmware::console::*;
}

/// 中断控制原语再导出：属于 **arch** 层（如 `sie` / `sstatus`），非 firmware。
pub mod interrupt {
    pub use arch::interrupt::*;
}

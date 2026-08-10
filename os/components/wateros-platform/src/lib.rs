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
//!   与平台 deadline timer 编程衔接），同时保存与机器无关的 IPI reason。
//!
//! 归属规则：本地 CSR、汇编和 trap 帧应落在 [`arch`]；SBI、DTB、QEMU machine
//! 约定和设备地址应落在 `platform-impl`；调度决策不能放入这两层。
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
/// Current StarFive VisionFive 2 implementation.
#[cfg(feature = "impl-jh7110-visionfive2")]
pub use impl_jh7110_visionfive2 as active_impl;
/// Current Loongson 2K1000LA implementation.
#[cfg(feature = "impl-loongson2k1000la")]
pub use impl_loongson2k1000la as active_impl;

/// 引导早期：保存平台持有的 DTB 物理指针（内存布局等平台侧解析使用）。
pub fn init_when_boot(dtb_pa: usize) {
    #[cfg(feature = "impl-qemu-riscv64-opensbi")]
    impl_qemu_riscv64_opensbi::dtb::store(dtb_pa);
    #[cfg(feature = "impl-qemu-loongarch64-virt")]
    impl_qemu_loongarch64_virt::dtb::store(dtb_pa);
    #[cfg(feature = "impl-jh7110-visionfive2")]
    impl_jh7110_visionfive2::dtb::store(dtb_pa);
    #[cfg(feature = "impl-loongson2k1000la")]
    {
        impl_loongson2k1000la::dtb::store(dtb_pa);
        // Discovery is deliberately fail-closed: a missing or unexpected PM
        // node leaves reset() unsupported instead of probing a fixed address.
        let _ = impl_loongson2k1000la::reset::discover_from_dtb(dtb_pa);
    }
    #[cfg(not(any(feature = "impl-qemu-riscv64-opensbi",
                  feature = "impl-qemu-loongarch64-virt",
                  feature = "impl-jh7110-visionfive2",
                  feature = "impl-loongson2k1000la")))]
    {
        let _ = dtb_pa;
    }
}

/// 平台持有的 DTB 物理指针（未保存时为 0）。
pub fn dtb_pa() -> usize {
    #[cfg(feature = "impl-qemu-riscv64-opensbi")]
    {
        impl_qemu_riscv64_opensbi::dtb::dtb_pa()
    }
    #[cfg(feature = "impl-qemu-loongarch64-virt")]
    {
        impl_qemu_loongarch64_virt::dtb::dtb_pa()
    }
    #[cfg(feature = "impl-jh7110-visionfive2")]
    { impl_jh7110_visionfive2::dtb::dtb_pa() }
    #[cfg(feature = "impl-loongson2k1000la")]
    { impl_loongson2k1000la::dtb::dtb_pa() }
    #[cfg(not(any(feature = "impl-qemu-riscv64-opensbi",
                  feature = "impl-qemu-loongarch64-virt",
                  feature = "impl-jh7110-visionfive2",
                  feature = "impl-loongson2k1000la")))]
    {
        0
    }
}

/// 物理 RAM 上界（不包含），用于恒等映射与帧分配器；QEMU 实现从平台持有的
/// DTB 解析，其它配置返回回退常量。
pub fn physical_ram_end_exclusive() -> usize {
    #[cfg(feature = "impl-qemu-riscv64-opensbi")]
    {
        impl_qemu_riscv64_opensbi::memory::physical_ram_end_exclusive()
    }
    #[cfg(feature = "impl-qemu-loongarch64-virt")]
    {
        impl_qemu_loongarch64_virt::memory::physical_ram_end_exclusive()
    }
    #[cfg(feature = "impl-jh7110-visionfive2")]
    { impl_jh7110_visionfive2::memory::physical_ram_end_exclusive() }
    #[cfg(feature = "impl-loongson2k1000la")]
    { impl_loongson2k1000la::memory::physical_ram_end_exclusive() }
    #[cfg(not(any(feature = "impl-qemu-riscv64-opensbi",
                  feature = "impl-qemu-loongarch64-virt",
                  feature = "impl-jh7110-visionfive2",
                  feature = "impl-loongson2k1000la")))]
    {
        config::mm::QEMU_VIRT_PHYS_RAM_END
    }
}

/// Board-provided RAM and identity-mapped MMIO layout consumed by the active
/// architecture MM implementation.
pub mod memory {
    pub use api_v0::memory::{KernelMemoryLayout, MemoryLayoutError, PhysicalRange};

    pub fn kernel_layout() -> KernelMemoryLayout {
        #[cfg(feature = "impl-qemu-riscv64-opensbi")]
        {
            return impl_qemu_riscv64_opensbi::memory::kernel_memory_layout();
        }
        #[cfg(all(not(feature = "impl-qemu-riscv64-opensbi"),
                  feature = "impl-qemu-loongarch64-virt"))]
        {
            return impl_qemu_loongarch64_virt::memory::kernel_memory_layout();
        }
        #[cfg(feature = "impl-jh7110-visionfive2")]
        { return impl_jh7110_visionfive2::memory::kernel_memory_layout(); }
        #[cfg(feature = "impl-loongson2k1000la")]
        { return impl_loongson2k1000la::memory::kernel_memory_layout(); }
        #[cfg(not(any(feature = "impl-qemu-riscv64-opensbi",
                      feature = "impl-qemu-loongarch64-virt",
                      feature = "impl-jh7110-visionfive2",
                      feature = "impl-loongson2k1000la")))]
        {
            const NO_MMIO : [PhysicalRange; 0] = [];
            KernelMemoryLayout { ram : PhysicalRange::new(config::mm::QEMU_VIRT_PHYS_RAM_BASE,
                                                           config::mm::QEMU_VIRT_PHYS_RAM_END),
                                 mmio : &NO_MMIO,
                                 probe_virtual_page : None }
        }
    }
}

/// 启动参数与引导上下文：具体类型由 feature 选中的 `platform-impl` 提供。
#[cfg(feature = "api-v0")]
pub mod boot;

/// 架构层再导出：trap、时间计数、中断控制、分页等与 ISA 直接相关的 API。
///
/// 与 **platform-impl** 的边界：仅操作 CPU / 监管态可见的硬件与 CSR，不选择板级后端。
pub mod arch {
    pub use arch::*;

    /// 执行当前 ISA 的极早初始化（例如安装 trap 向量）。
    ///
    /// BOOT_CONTRACT: 必须在打开全局中断之前调用；可重复性由各 arch impl 决定，
    /// 调用方不应把它当成可在运行期反复执行的初始化接口。
    pub fn init() {
        arch_boot();
    }
}

/// Platform SMP lifecycle. Only the RISC-V OpenSBI profile currently starts
/// secondaries; other profiles deliberately report `Unsupported`.
#[cfg(feature = "api-v0")]
pub mod smp;

/// 平台层时间频率：由 `PlatformTime` 实现（通常来自板级 / DTB / 环境常量），
/// **不**等同于 arch 的 `time` CSR 读频率（arch 侧可能返回不支持）。
///
/// 引导期可通过 [`set_frequency_hz`] 注入 DTB 探测结果；未设置时回退
/// `PlatformTimeImpl::get_time_frequency_hz`。
#[cfg(feature = "api-v0")]
pub mod time;

/// 组合定时器：用 arch 的 tick 与 [`crate::time`] 给出的 Hz 换算时刻，再经
/// 平台后端编程下一次定时器中断。
///
/// 错误变体区分 `Arch` / `Platform` / `DeadlineTimer` 三层来源，便于定位是 CSR、
/// 频率配置还是平台定时器后端调用失败。
pub mod timer {
    use core::time::Duration;

    pub use api_v0::time::PlatformTimeError;
    pub use api_v0::timer::{
        PlatformDeadlineTimerError, PlatformDeadlineTimerResult, PlatformTimerDeadline,
    };
    pub use arch::time::{ArchTimeError, ArchTimeFrequency, ArchTimeTick};

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

    /// 读取 arch 单调计数器的原始 tick，不进行频率换算。
    ///
    /// TIME_CONTRACT: 返回值只在同一 arch tick 源内可比较，不能直接与 scheduler
    /// 的软件 tick、wall-clock 纳秒或其他 CPU 的不同计数源混用。
    #[inline]
    pub fn now_tick() -> PlatformTimerResult<ArchTimeTick> {
        arch::time::read_time_tick().map_err(PlatformTimerError::Arch)
    }

    /// 取得用于将 arch tick 换算为时间的频率。
    ///
    /// 频率来源由 [`crate::time`] 管理；错误意味着当前阶段尚未配置可信 timebase。
    #[inline]
    pub fn tick_hz() -> PlatformTimerResult<ArchTimeFrequency> {
        let hz = crate::time::frequency_hz().map_err(PlatformTimerError::Platform)?;
        Ok(ArchTimeFrequency(hz))
    }

    /// 将绝对 arch tick deadline 交给当前 platform profile。
    ///
    /// PLATFORM_BOUNDARY: 此处不写 CSR；RISC-V 可能经 SBI，其他 profile 可以经
    /// machine timer/MMIO。调用方必须确保 deadline 与 [`now_tick`] 同一刻度。
    #[inline]
    pub fn set_timer_deadline_tick(deadline_tick: ArchTimeTick) -> PlatformTimerResult<()> {
        crate::active_impl::timer::set_timer(PlatformTimerDeadline(deadline_tick.0))
            .map_err(PlatformTimerError::DeadlineTimer)
    }

    /// 将 duration 向上取整为 tick，保证请求不会因截断而提前触发。
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

    /// 将原始 tick 换算为 duration；仅用于观测，纳秒精度会按整数除法向下截断。
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

    /// 返回当前单调时间；不会推进 scheduler 时间，也不会编程下一次中断。
    #[inline]
    pub fn now_duration() -> PlatformTimerResult<Duration> {
        let tick = now_tick()?.0;
        let hz = tick_hz()?.0;
        ticks_to_duration(tick, hz)
    }

    /// 在当前 CPU 上编程“至少经过 `d` 后”触发的下一次 timer interrupt。
    ///
    /// TIME_CONTRACT: 这是本地硬件 deadline；全局 sleep/wait timeout 是否由该 CPU
    /// 推进由 `wateros-task` 决定，AP 不得借此重复推进 BSP 的全局 timekeeper。
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

    /// [`set_timer_after`] 的毫秒便捷封装。
    #[inline]
    pub fn set_timer_after_ms(ms: u64) -> PlatformTimerResult<()> {
        set_timer_after(Duration::from_millis(ms))
    }

    /// [`set_timer_after`] 的秒便捷封装。
    #[inline]
    pub fn set_timer_after_s(s: u64) -> PlatformTimerResult<()> {
        set_timer_after(Duration::from_secs(s))
    }
}

/// 系统复位与关机：由当前 platform impl 提供，不经由 arch 特权指令封装。
pub mod reset {
    pub use crate::active_impl::reset::{reboot, reset, shutdown};
    pub use api_v0::reset::{
        PlatformResetError, PlatformResetReason, PlatformResetResult, PlatformResetType,
    };
}

/// 早期控制台输出：由当前 board feature 选择 OpenSBI、UART 或其它平台后端。
pub mod console {
    pub use api_v0::console::{PlatformConsoleError, PlatformConsoleResult};
    use base::sync::MultiprocessorSafeCell;
    use core::sync::atomic::{AtomicUsize, Ordering};

    /// 写控制台时临时屏蔽本核中断，并在离开时恢复原始状态。
    ///
    /// IPI_SYNC: 这只避免同一 CPU 的中断重入；跨 CPU 串行化由
    /// [`CONSOLE_WRITE_LOCK`] 完成。不要在持有 scheduler 锁时调用控制台。
    struct ConsoleInterruptGuard(Option<arch::interrupt::ArchInterruptState>);

    impl ConsoleInterruptGuard {
        #[inline]
        fn new() -> Self {
            let state = arch::interrupt::read_global_interrupt_state().ok();
            let _ = arch::interrupt::disable_global_interrupt();
            Self(state)
        }
    }

    impl Drop for ConsoleInterruptGuard {
        fn drop(&mut self) {
            if let Some(state) = self.0 {
                let _ = arch::interrupt::restore_global_interrupt_state(state);
            }
        }
    }

    /// 运行期可选的控制台接收端。OS 在完整 UART 字符设备注册后安装它；
    /// 在此之前必须保持 `None`，以便 early console 仍能用于引导日志。
    pub type RuntimeConsoleWriter = fn(&[u8]) -> PlatformConsoleResult<()>;
    /// driver 层注册后的运行期 writer；`None` 时回退到 early-console profile。
    static RUNTIME_CONSOLE_WRITER: MultiprocessorSafeCell<Option<RuntimeConsoleWriter>> =
        MultiprocessorSafeCell::new(None);
    /// 串行化跨 CPU 的整段输出，覆盖 runtime UART 尚未注册的 early boot 阶段。
    static CONSOLE_WRITE_LOCK: MultiprocessorSafeCell<()> = MultiprocessorSafeCell::new(());
    const NO_CONSOLE_OWNER: usize = usize::MAX;
    /// 当前持锁 CPU，用于识别同 CPU 的嵌套日志并避免递归获取自旋锁。
    static CONSOLE_WRITE_OWNER: AtomicUsize = AtomicUsize::new(NO_CONSOLE_OWNER);

    /// 安装运行期控制台写入端。
    ///
    /// BOOT_CONTRACT: 只能在字符设备和其内部锁已完全初始化后调用；替换 writer 时
    /// 必须由启动序列保证没有并发输出，当前接口不提供注销或热替换同步。
    pub fn register_runtime_writer(writer: RuntimeConsoleWriter) {
        *RUNTIME_CONSOLE_WRITER.exclusive_access() = Some(writer);
    }

    /// 写一个字节；换行转换和后端错误语义由选中的 console profile 决定。
    #[inline]
    fn runtime_writer() -> Option<RuntimeConsoleWriter> {
        *RUNTIME_CONSOLE_WRITER.exclusive_access()
    }

    /// 在中断屏蔽和跨 CPU 输出锁保护下执行一次完整写入。
    ///
    /// `write(true)` 表示当前 CPU 已递归持锁：此时只能走 early profile，不能再次
    /// 获取 runtime writer 的锁，避免格式化日志重入导致死锁。
    fn with_console_write_lock<R>(write: impl FnOnce(bool) -> R) -> R {
        let _interrupt_guard = ConsoleInterruptGuard::new();
        let cpu = arch::cpu::current_cpu_id().raw();
        let guard = match CONSOLE_WRITE_LOCK.try_lock() {
            Some(guard) => guard,
            None if CONSOLE_WRITE_OWNER.load(Ordering::Acquire) == cpu => {
                return write(true);
            }
            None => CONSOLE_WRITE_LOCK.exclusive_access(),
        };

        CONSOLE_WRITE_OWNER.store(cpu, Ordering::Release);
        let result = write(false);
        CONSOLE_WRITE_OWNER.store(NO_CONSOLE_OWNER, Ordering::Release);
        drop(guard);
        result
    }

    /// 原子性边界为整个 `bytes` 缓冲，而不是单个字符，避免多核日志行互相穿插。
    #[inline]
    pub fn console_write_a_byte(byte: u8) -> PlatformConsoleResult<()> {
        console_write_a_buffer(core::slice::from_ref(&byte))
    }

    /// 请求后端将已经写入的字节送出；不隐含全局内存屏障或设备驱动 drain。
    #[inline]
    pub fn console_write_a_buffer(bytes: &[u8]) -> PlatformConsoleResult<()> {
        with_console_write_lock(|reentrant| {
            if !reentrant {
                if let Some(writer) = runtime_writer() {
                    return writer(bytes);
                }
            }
            crate::active_impl::console::console_write_a_buffer(bytes)
        })
    }

    /// Write exact terminal wire bytes under the same cross-CPU lock used by
    /// kernel logging. No CR/LF conversion is performed by the board backend.
    pub fn console_write_raw_buffer(bytes: &[u8]) -> PlatformConsoleResult<()> {
        with_console_write_lock(|reentrant| {
            if !reentrant {
                if let Some(writer) = runtime_writer() {
                    return writer(bytes);
                }
            }
            crate::active_impl::console::console_write_raw_buffer(bytes)
        })
    }

    /// 在同一次底层控制台锁持有期间完成完整格式化操作。
    ///
    /// `fmt::Write` 可以多次调用 `write_str`，因此不能把锁放在 `Writer` 内的每次
    /// 回调中，否则一条格式化日志仍可能被别的 CPU 插入。
    pub fn console_write_fmt(args: core::fmt::Arguments<'_>) -> PlatformConsoleResult<()> {
        struct Writer(Option<RuntimeConsoleWriter>);
        impl core::fmt::Write for Writer {
            fn write_str(&mut self, value: &str) -> core::fmt::Result {
                if let Some(writer) = self.0 {
                    writer(value.as_bytes()).map_err(|_| core::fmt::Error)
                } else {
                    crate::active_impl::console::console_write_a_buffer(value.as_bytes())
                        .map_err(|_| core::fmt::Error)
                }
            }
        }

        with_console_write_lock(|reentrant| {
            let writer = if reentrant { None } else { runtime_writer() };
            core::fmt::Write::write_fmt(&mut Writer(writer), args)
                .map_err(|_| PlatformConsoleError::WriteFailure)
        })
    }

    #[inline]
    pub fn console_flush() -> PlatformConsoleResult<()> {
        crate::active_impl::console::console_flush()
    }
}

/// 墙上时钟（`CLOCK_REALTIME` 偏移 + 单调时钟）。
pub mod wall_clock;

/// 中断控制原语再导出：属于 **arch** 层（如 `sie` / `sstatus`），非 platform impl。
pub mod interrupt {
    pub use arch::interrupt::*;
}

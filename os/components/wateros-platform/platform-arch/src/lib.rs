//! **架构（ISA）层**：聚合 `arch-api-v0` 与具体 `arch-impl-*`，向内核暴露
//! trap、 时间计数、任务硬件上下文、中断位、分页 CSR 等与指令集强相关的原语。
//!
//! ## 与 `wateros-platform-impl` 的边界
//! - 本 crate **仅**操作 CPU 可见的 CSR/汇编约定（如 RISC-V `stimecmp`
//!   若将来直接访问也属于 arch 讨论范围）；**经 SBI 设置下次中断时刻**、
//!   **经固件写串口** 等属于 platform profile。
//! - trap 路径中的定时器重载、调度 tick、syscall 分发等业务由组合层通过
//!   `arch-api::kernel_trap` 接入；arch impl 只负责 trap 向量、帧布局和 CSR
//!   语义。
//!
//! 特别地，IPI 的发送属于 profile 的运输层；而目标 CPU 在 trap 中清除本地
//! SSIP/IOCSR pending 位属于本 crate 的 `interrupt` 模块。
#![no_std]
#[cfg(all(feature = "impl-riscv64", feature = "impl-loongarch64"))]
compile_error!("select only one platform-arch implementation");


/// 极早启动钩子：在具备 trap 能力前初始化与 trap 向量相关的一小部分 arch 状态。
///
/// 当前实现：在启用 `api-v0` 与 `impl-riscv64` 时安装 trap 向量；其它 feature
/// 组合下可能为空操作或扩展点。
#[unsafe(no_mangle)]
pub fn arch_boot() {
    #[cfg(feature = "impl-riscv64")]
    #[cfg(feature = "api-v0")]
    impl_riscv64::init_trap();

    #[cfg(feature = "impl-loongarch64")]
    #[cfg(feature = "api-v0")]
    impl_loongarch64::init_trap();
}

/// RISC-V `time` / `timeh` 等计数与频率查询（频率在实现中可返回不支持，由
/// platform 层补全）。
#[cfg(feature = "api-v0")]
pub mod time {
    pub use api_v0::time::{
        ArchTime, ArchTimeError, ArchTimeFrequency, ArchTimeResult, ArchTimeTick,
    };

    #[cfg(feature = "impl-loongarch64")]
    pub use impl_loongarch64::time::LoongArch64ArchTime as ArchTimeImpl;
    #[cfg(feature = "impl-riscv64")]
    pub use impl_riscv64::time::Riscv64ArchTime as ArchTimeImpl;

    #[inline]
    pub fn read_time_tick() -> ArchTimeResult<ArchTimeTick> { ArchTimeImpl::read_time_tick() }

    #[inline]
    pub fn read_time_frequency() -> ArchTimeResult<ArchTimeFrequency> {
        ArchTimeImpl::read_time_frequency()
    }
}

/// 任务切换保存的最小寄存器集合抽象（具体布局由 `impl-riscv64` 等决定）。
#[cfg(feature = "api-v0")]
pub mod task {
    pub use api_v0::task::ArchTaskContext;

    #[cfg(feature = "impl-loongarch64")]
    pub use impl_loongarch64::task::LoongArch64ArchTaskContext as ActiveArchTaskContext;
    #[cfg(feature = "impl-riscv64")]
    pub use impl_riscv64::task::Riscv64ArchTaskContext as ActiveArchTaskContext;
}

/// 异常与中断上下文：trap 帧读写、系统调用参数视图等（ISA 语义，非固件 ABI）。
#[cfg(feature = "api-v0")]
pub mod trap {
    #[allow(deprecated)]
    pub use api_v0::trap::{Exception, Interrupt, TrapCause, TrapFrameRead, TrapFrameWrite};

    #[cfg(feature = "impl-loongarch64")]
    pub use impl_loongarch64::trap::TrapContext as ActiveTrapFrame;
    #[cfg(feature = "impl-riscv64")]
    pub use impl_riscv64::trap::TrapContext as ActiveTrapFrame;

    #[cfg(feature = "impl-loongarch64")]
    pub use impl_loongarch64::trap::prepare_user_trap_frame_access;
    #[cfg(feature = "impl-riscv64")]
    pub use impl_riscv64::trap::prepare_user_trap_frame_access;

    #[cfg(feature = "impl-loongarch64")]
    pub use impl_loongarch64::trap::user_trap_requires_kernel_address_space;
    #[cfg(feature = "impl-riscv64")]
    pub use impl_riscv64::trap::user_trap_requires_kernel_address_space;

    #[cfg(feature = "impl-loongarch64")]
    pub use impl_loongarch64::trap::timer_slice_ticks;
    #[cfg(feature = "impl-riscv64")]
    pub use impl_riscv64::trap::timer_slice_ticks;

    #[cfg(feature = "impl-riscv64")]
    pub use impl_riscv64::trap::set_kernel_trap_satp;
}

/// 当前 CPU id。
#[cfg(feature = "api-v0")]
pub mod cpu {
    pub use api_v0::cpu::{ArchCpuInitError, ArchCpuInitResult};
    #[cfg(feature = "impl-loongarch64")]
    pub use impl_loongarch64::cpu::{current_cpu_id, init_current_cpu};
    #[cfg(feature = "impl-riscv64")]
    pub use impl_riscv64::cpu::{current_cpu_id, init_current_cpu};
}

/// 核间中断（IPI）：发送 Supervisor Soft Interrupt 至目标 CPU。
///
/// RISC-V 下通过 SBI `send_ipi` 实现；LoongArch 使用 IOCSR/IPI 寄存器。
#[cfg(feature = "api-v0")]
pub mod ipi {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum IpiError {
        Firmware(usize),
        Unsupported,
    }

    /// 向 `cpu_mask` 指定的所有 CPU 发送核间中断。
    #[cfg(feature = "impl-riscv64")]
    pub fn send_ipi(cpu_mask : base::cpu::CpuMask) -> Result<(), IpiError> {
        impl_riscv64::ipi::send_ipi(cpu_mask).map_err(|error| match error {
                                                 impl_riscv64::ipi::IpiError::Firmware(code) => {
                                                     IpiError::Firmware(code)
                                                 }
                                                 impl_riscv64::ipi::IpiError::Unsupported => {
                                                     IpiError::Unsupported
                                                 }
                                             })
    }
    #[cfg(not(feature = "impl-riscv64"))]
    pub fn send_ipi(_cpu_mask : base::cpu::CpuMask) -> Result<(), IpiError> {
        Err(IpiError::Unsupported)
    }
}

/// 监管态中断屏蔽与使能（如 `sie` / `sstatus.SIE`），**不**包含对 CLINT/ACLINT
/// 或 SBI `set_timer` 的编程。
#[cfg(feature = "api-v0")]
pub mod interrupt {
    pub use api_v0::interrupt::ArchTimerInterruptControl;
    pub use api_v0::time::ArchTimeResult;

    #[cfg(feature = "impl-loongarch64")]
    pub use impl_loongarch64::interrupt::LoongArch64ArchInterrupt as ArchInterruptImpl;
    #[cfg(feature = "impl-riscv64")]
    pub use impl_riscv64::interrupt::Riscv64ArchInterrupt as ArchInterruptImpl;

    pub use api_v0::interrupt::ArchInterruptState;

    #[inline]
    pub fn enable_timer_interrupt() -> ArchTimeResult<()> {
        ArchInterruptImpl::enable_timer_interrupt()
    }

    #[inline]
    pub fn disable_timer_interrupt() -> ArchTimeResult<()> {
        ArchInterruptImpl::disable_timer_interrupt()
    }

    #[inline]
    pub fn enable_global_interrupt() -> ArchTimeResult<()> {
        ArchInterruptImpl::enable_global_interrupt()
    }

    #[inline]
    pub fn disable_global_interrupt() -> ArchTimeResult<()> {
        ArchInterruptImpl::disable_global_interrupt()
    }

    #[inline]
    pub fn read_global_interrupt_state() -> ArchTimeResult<ArchInterruptState> {
        ArchInterruptImpl::read_global_interrupt_state()
    }

    #[inline]
    pub fn restore_global_interrupt_state(state : ArchInterruptState) -> ArchTimeResult<()> {
        ArchInterruptImpl::restore_global_interrupt_state(state)
    }

    #[inline]
    pub fn wait_for_interrupt() { ArchInterruptImpl::wait_for_interrupt(); }

    #[inline]
    pub fn clear_soft_interrupt() {
        #[cfg(feature = "impl-riscv64")]
        impl_riscv64::interrupt::clear_soft_interrupt();
        #[cfg(feature = "impl-loongarch64")]
        impl_loongarch64::interrupt::clear_soft_interrupt();
    }

    #[inline]
    pub fn enable_soft_interrupt() {
        #[cfg(feature = "impl-riscv64")]
        impl_riscv64::interrupt::enable_soft_interrupt();
        #[cfg(feature = "impl-loongarch64")]
        impl_loongarch64::interrupt::enable_soft_interrupt();
    }

    #[inline]
    pub fn disable_soft_interrupt() {
        #[cfg(feature = "impl-riscv64")]
        impl_riscv64::interrupt::disable_soft_interrupt();
        #[cfg(feature = "impl-loongarch64")]
        impl_loongarch64::interrupt::disable_soft_interrupt();
    }
}

/// 地址空间激活与必要的地址翻译缓存刷新原语；页表内容在 MM 组件。
///
/// 分页控制原语：只负责读写 ISA CSR、开关 MMU、刷新 TLB/地址翻译缓存。具体页表
/// 格式、PTE 编码、地址空间构造仍由 `wateros-mm/mm-impl/*/pagetable.rs` 负责。
///
/// RISC-V 的 token 是 `satp` 编码值；其它架构可使用自己的根页表/ASID 编码。
///
/// 内核自身的地址空间 token 由 MM 层维护并通过 [`mm::kernel_mm::kernel_satp`]
/// 提供。
pub mod paging {
    pub use api_v0::paging::TlbFlushRange;
    #[cfg(feature = "impl-loongarch64")]
    pub use impl_loongarch64::paging::LoongArch64Paging as ArchPagingImpl;
    #[cfg(feature = "impl-riscv64")]
    pub use impl_riscv64::paging::Riscv64Paging as ArchPagingImpl;

    /// 读取当前激活的地址空间 token。
    #[inline]
    pub fn active_address_space_token() -> usize { ArchPagingImpl::active_address_space_token() }

    /// 初始化并返回硬件实际实现的地址空间标识位数。
    ///
    /// RISC-V 通过 `satp` WARL 规则探测；LoongArch64 返回架构固定宽度。
    #[inline]
    pub fn initialize_address_space_ids() -> usize {
        ArchPagingImpl::initialize_address_space_ids()
    }

    /// 切换地址空间 token 并刷新本地翻译缓存。
    #[inline]
    pub fn activate_address_space_token_and_flush(token : usize) {
        ArchPagingImpl::activate_address_space_token_and_flush(token)
    }

    /// 当前地址空间下 PTE 已就地修改时，刷新本地 CPU/hart 的地址翻译缓存。
    #[inline]
    pub fn flush_address_space_translations() { ArchPagingImpl::flush_address_space_translations() }

    /// 刷新当前 CPU 的地址翻译；若 ISA 没有等价指令，架构实现可保守扩大范围。
    #[inline]
    pub fn flush_tlb_local(range : TlbFlushRange) { ArchPagingImpl::flush_tlb_local(range) }

    /// 关闭 MMU（CRMD.PG = 0），在构建内核页表前确认为直接物理寻址。
    #[inline]
    pub fn init_paging_disable_mmu() { ArchPagingImpl::init_paging_disable_mmu(); }

    /// 开启 MMU（CRMD.PG = 1），在页表构建完成后与
    /// activate_address_space_token_and_flush 配合使用。
    #[inline]
    pub fn enable_paging() { ArchPagingImpl::enable_paging(); }
}

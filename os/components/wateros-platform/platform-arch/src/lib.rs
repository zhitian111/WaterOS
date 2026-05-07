#![no_std]

//! **架构（ISA）层**：聚合 `arch-api-v0` 与具体 `arch-impl-*`，向内核暴露 trap、
//! 时间计数、任务硬件上下文、中断位、分页 CSR 等与指令集强相关的原语。
//!
//! ## 与 `wateros-platform-firmware` 的边界
//! - 本 crate **仅**操作 CPU 可见的 CSR/汇编约定（如 RISC-V `stimecmp` 若将来直接
//!   访问也属于 arch 讨论范围）；**经 SBI 设置下次中断时刻**、**经固件写串口** 等
//!   属于 firmware 层，由依赖方分别引用 `arch` 与 `firmware` crate 组合使用。
//! - `arch-impl-riscv64` 的 trap 路径可能**调用** firmware 以重载定时器，这是
//!   **组合点**而非“arch API 向外暴露 SBI”：对外仍通过 `firmware` 的 trait 实现隔离。

/// 极早启动钩子：在具备 trap 能力前初始化与 trap 向量相关的一小部分 arch 状态。
///
/// 当前实现：在启用 `api-v0` 与 `impl-riscv64` 时安装 trap 向量；其它 feature
/// 组合下可能为空操作或扩展点。
#[unsafe(no_mangle)]
pub fn arch_boot() {
    #[cfg(feature = "impl-riscv64")]
    #[cfg(feature = "api-v0")]
    impl_riscv64::init_trap();
}

/// RISC-V `time` / `timeh` 等计数与频率查询（频率在实现中可返回不支持，由 platform 层补全）。
#[cfg(feature = "api-v0")]
pub mod time {
    pub use api_v0::time::{
        ArchTime, ArchTimeError, ArchTimeFrequency, ArchTimeResult, ArchTimeTick,
    };

    #[cfg(feature = "impl-riscv64")]
    pub use impl_riscv64::time::Riscv64ArchTime as ArchTimeImpl;

    #[inline]
    pub fn read_time_tick() -> ArchTimeResult<ArchTimeTick> {
        ArchTimeImpl::read_time_tick()
    }

    #[inline]
    pub fn read_time_frequency() -> ArchTimeResult<ArchTimeFrequency> {
        ArchTimeImpl::read_time_frequency()
    }
}

/// 任务切换保存的最小寄存器集合抽象（具体布局由 `impl-riscv64` 等决定）。
#[cfg(feature = "api-v0")]
pub mod task {
    pub use api_v0::task::ArchTaskContext;

    #[cfg(feature = "impl-riscv64")]
    pub use impl_riscv64::task::Riscv64ArchTaskContext as ActiveArchTaskContext;
}

/// 异常与中断上下文：trap 帧读写、系统调用参数视图等（ISA 语义，非固件 ABI）。
#[cfg(feature = "api-v0")]
pub mod trap {
    #[allow(deprecated)]
    pub use api_v0::trap::{
        ArchTrapFrame, Exception, Interrupt, TrapCOntextWrite, TrapCause, TrapContextFrameView,
        TrapContextRead, TrapContextWrite, TrapFrame, TrapFrameRead, TrapFrameWrite,
        TrapSyscallRead, TrapSyscallWrite,
    };

    #[cfg(feature = "impl-riscv64")]
    pub use impl_riscv64::trap::TrapContext as ActiveTrapFrame;
}

/// 监管态中断屏蔽与使能（如 `sie` / `sstatus.SIE`），**不**包含对 CLINT/ACLINT 或
/// SBI `set_timer` 的编程。
#[cfg(feature = "api-v0")]
pub mod interrupt {
    pub use api_v0::interrupt::ArchTimerInterruptControl;
    pub use api_v0::time::ArchTimeResult;

    #[cfg(feature = "impl-riscv64")]
    pub use impl_riscv64::interrupt::Riscv64ArchInterrupt as ArchInterruptImpl;

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
}

/// 分页控制 CSR（如 `satp`）与必要的 TLB 刷新原语；页表内容管理在上层 MM 组件。
pub mod paging {
    #[cfg(feature = "impl-riscv64")]
    pub use impl_riscv64::paging::Riscv64Paging as ArchPagingImpl;

    #[inline]
    pub fn read_satp() -> usize {
        ArchPagingImpl::read_satp()
    }

    #[inline]
    pub fn write_satp_and_flush(satp: usize) {
        ArchPagingImpl::write_satp_and_flush(satp)
    }
}

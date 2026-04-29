#![no_std]
#[unsafe(no_mangle)]
pub fn arch_boot() {
    #[cfg(feature = "impl-riscv64")]
    #[cfg(feature = "api-v0")]
    impl_riscv64::init_trap();
}

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

#[cfg(feature = "api-v0")]
pub mod task {
    pub use api_v0::task::ArchTaskContext;

    #[cfg(feature = "impl-riscv64")]
    pub use impl_riscv64::task::Riscv64ArchTaskContext as ActiveArchTaskContext;
}

#[cfg(feature = "api-v0")]
pub mod trap {
    pub use api_v0::trap::{
        ArchTrapFrame, Exception, Interrupt, TrapCOntextWrite, TrapCause, TrapContextFrameView,
        TrapContextRead, TrapContextWrite,
    };

    #[cfg(feature = "impl-riscv64")]
    pub use impl_riscv64::trap::TrapContext as ActiveTrapFrame;
}

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

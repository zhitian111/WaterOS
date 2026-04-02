use api_v0::interrupt::ArchTimerInterruptControl;
use api_v0::time::ArchTimeResult;
use riscv::register::{sie, sstatus};

/// RISC-V 架构中断控制实现（与时间计数读取解耦）。
pub struct Riscv64ArchInterrupt;

impl ArchTimerInterruptControl for Riscv64ArchInterrupt {
    #[inline]
    fn enable_timer_interrupt() -> ArchTimeResult<()> {
        unsafe {
            sie::set_stimer();
        }
        Ok(())
    }

    #[inline]
    fn disable_timer_interrupt() -> ArchTimeResult<()> {
        unsafe {
            sie::clear_stimer();
        }
        Ok(())
    }

    #[inline]
    fn enable_global_interrupt() -> ArchTimeResult<()> {
        unsafe {
            sstatus::set_sie();
        }
        Ok(())
    }

    #[inline]
    fn disable_global_interrupt() -> ArchTimeResult<()> {
        unsafe {
            sstatus::clear_sie();
        }
        Ok(())
    }
}


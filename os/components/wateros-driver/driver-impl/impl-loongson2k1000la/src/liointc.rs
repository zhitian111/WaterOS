//! Loongson 2K1000 LIOINTC 外部中断接入（复用 `irq-loongarch`）。
//!
//! LIOINTC 路由到 core0/HWI0（ESTAT bit 2），与 WaterOS 的 IPI（bit 12）/Timer
//! （bit 11）不冲突。第一阶段无设备 handler：claim 后记日志并 complete；真机验证后
//! 接设备派发表。

use api_v0::{DriverError, DriverResult};
use core::sync::atomic::{AtomicBool, Ordering};

/// Linux 2K1000 DTS 的 LIOINTC 固定基址（真机确认前为回退值）。
pub const LIOINTC_MAIN_BASE : usize = 0x1fe0_1400;
/// LIOINTC core0 中断状态（ISR0）寄存器基址。
pub const LIOINTC_ISR0_BASE : usize = 0x1fe0_1540;
const MAX_IRQS : u32 = 64;

/// 抽象 claim/complete，便于 host 单测闭环；真实实现包住 `irq-loongarch::Liointc`。
pub trait ClaimComplete {
    fn claim(&self) -> Option<u32>;
    fn complete(&self, irq : u32);
}

#[cfg(target_arch = "loongarch64")]
struct RealController(irq_loongarch::liointc::Liointc);

#[cfg(target_arch = "loongarch64")]
impl ClaimComplete for RealController {
    fn claim(&self) -> Option<u32> {
        self.0
            .claim_irq()
            .map(|irq| irq as u32)
    }

    fn complete(&self, irq : u32) {
        self.0.complete_irq(irq as usize);
    }
}

#[cfg(target_arch = "loongarch64")]
static CONTROLLER : RealController = RealController(unsafe {
    irq_loongarch::liointc::Liointc::new(LIOINTC_MAIN_BASE, LIOINTC_ISR0_BASE)
});
#[cfg(target_arch = "loongarch64")]
static INIT_DONE : AtomicBool = AtomicBool::new(false);

/// 初始化本 CPU 的 LIOINTC（BSP 上执行一次 init；AP 复用已配置控制器）。
pub fn init_current_cpu(_cpu_raw : usize) -> DriverResult<()> {
    #[cfg(target_arch = "loongarch64")]
    {
        if !INIT_DONE.swap(true, Ordering::AcqRel) {
            CONTROLLER.0.init();
            log::info!("[driver][2k1000] LIOINTC initialized (main={:#x} isr0={:#x})",
                       LIOINTC_MAIN_BASE,
                       LIOINTC_ISR0_BASE);
        }
        Ok(())
    }
    #[cfg(not(target_arch = "loongarch64"))]
    {
        let _ = _cpu_raw;
        Err(DriverError::Unsupported)
    }
}

/// 处理一次已 claim 的中断：当前无设备 handler，记日志；返回是否应 complete。
pub fn dispatch_claimed_irq(irq : u32) -> bool {
    if irq >= MAX_IRQS {
        log::warn!("[driver][2k1000] LIOINTC out-of-range irq {}", irq);
        return false;
    }
    log::info!("[driver][2k1000] LIOINTC irq {} (no handler; completing)",
               irq);
    true
}

/// 处理一次机器外部中断：claim → 派发 → complete。
pub fn handle_external_interrupt<C : ClaimComplete>(controller : &C) -> DriverResult<bool> {
    let Some(irq) = controller.claim() else {
        return Ok(false);
    };
    if dispatch_claimed_irq(irq) {
        controller.complete(irq);
    }
    Ok(true)
}

/// LoongArch 目标上的实际入口（使用静态 LIOINTC 控制器）。
#[cfg(target_arch = "loongarch64")]
pub fn handle_external_interrupt_la() -> DriverResult<bool> {
    handle_external_interrupt(&CONTROLLER)
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::Cell;

    struct MockController {
        pending : Cell<Option<u32>>,
        completed : Cell<Option<u32>>,
    }

    impl ClaimComplete for MockController {
        fn claim(&self) -> Option<u32> {
            self.pending.replace(None)
        }

        fn complete(&self, irq : u32) {
            self.completed.set(Some(irq));
        }
    }

    #[test]
    fn claim_dispatch_complete_roundtrip() {
        let controller = MockController { pending : Cell::new(Some(7)),
                                          completed : Cell::new(None) };
        assert_eq!(handle_external_interrupt(&controller), Ok(true));
        assert_eq!(controller.completed.get(), Some(7));
    }

    #[test]
    fn empty_claim_reports_no_pending_source() {
        let controller = MockController { pending : Cell::new(None),
                                          completed : Cell::new(None) };
        assert_eq!(handle_external_interrupt(&controller), Ok(false));
        assert_eq!(controller.completed.get(), None);
    }

    #[test]
    fn out_of_range_irq_is_not_completed() {
        assert!(!dispatch_claimed_irq(MAX_IRQS));
        assert!(dispatch_claimed_irq(0));
    }
}

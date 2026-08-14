//! WaterOS 自研 IRQ 核心（Linux genirq 语义的薄版）。
//!
//! - [`chip`]：`IrqChip` trait，各平台/板级中断控制器（PLIC、EIOINTC 等）实现。
//! - [`domain`]：`(chip, hwirq) -> virq` 的运行期注册与解析。
//! - [`action`]：中断处理器注册表（top-half / bottom-half 绑定）。
//!
//! 本 crate **不依赖** `wateros-platform` / `wateros-task`，保证平台实现可以反向
//! 依赖它；DTB 的静态中断解析在 `wateros-driver-impl-common`，两者在设备注册阶段
//! 对接（DTB 描述 -> [`domain::register_line`] -> 驱动持有 virq）。

#![no_std]
extern crate alloc;

pub mod action;
pub mod bottom_half;
pub mod chip;
pub mod domain;
pub mod types;

pub use types::{HwIrq, IrqAffinity, IrqError, IrqResult, IrqTrigger, Virq};

/// 自检：验证注册表基本不变量（内核 self_test 路径调用）。
#[cfg(feature = "self_test")]
pub fn self_test() {
    use crate::action::IrqReturn;
    use crate::chip::IrqChip;
    use crate::types::HwIrq;
    use core::sync::atomic::{AtomicUsize, Ordering};

    struct DummyChip;
    impl IrqChip for DummyChip {
        fn name(&self) -> &'static str { "dummy" }
        fn enable(&self, _irq : HwIrq) -> IrqResult<()> { Ok(()) }
        fn disable(&self, _irq : HwIrq) -> IrqResult<()> { Ok(()) }
    }

    fn dummy_handler(_virq : Virq, _dev_id : usize) -> IrqReturn { IrqReturn::Handled }

    static BH_RAN : AtomicUsize = AtomicUsize::new(0);
    fn bh_handler(_virq : Virq, _dev_id : usize) { BH_RAN.fetch_add(1, Ordering::SeqCst); }

    static CHIP : DummyChip = DummyChip;
    let virq =
        domain::register_line(HwIrq(7), &CHIP, IrqTrigger::LevelHigh).expect("register line in \
                                                                              self_test");
    assert!(domain::line(virq).is_some(),
            "registered line must resolve");
    let handle = action::request_irq(virq, dummy_handler, 0x1234).expect("request irq");
    assert!(action::action(handle).is_some(),
            "action must be queryable");
    assert!(action::free_irq(handle),
            "free must succeed once");
    assert!(action::action(handle).is_none(),
            "freed action must be gone");

    let bh_virq =
        domain::register_line(HwIrq(9), &CHIP, IrqTrigger::EdgeRising).expect("register bh line \
                                                                               in self_test");
    let bh_handle =
        action::request_irq_with_bottom_half(bh_virq, dummy_handler, bh_handler, 0x55)
            .expect("request bh irq in self_test");
    assert_eq!(action::dispatch(bh_virq),
               IrqReturn::Handled,
               "top-half must handle");
    assert!(bottom_half::has_pending(),
            "dispatch must schedule bottom-half");
    bottom_half::run_pending();
    assert_eq!(BH_RAN.load(Ordering::SeqCst),
               1,
               "bottom-half must run once");
    assert!(!bottom_half::has_pending(),
            "queue must drain");
    assert!(action::free_irq(bh_handle));

    log::info!("[irq] self_test ok: virq={:?}", virq);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::IrqReturn;
    use crate::chip::IrqChip;
    use core::sync::atomic::{AtomicUsize, Ordering};

    struct DummyChip;
    impl IrqChip for DummyChip {
        fn name(&self) -> &'static str { "dummy" }
        fn enable(&self, _irq : HwIrq) -> IrqResult<()> { Ok(()) }
        fn disable(&self, _irq : HwIrq) -> IrqResult<()> { Ok(()) }
    }

    fn dummy_handler(_virq : Virq, _dev_id : usize) -> IrqReturn { IrqReturn::Handled }

    #[test]
    fn core_roundtrip() {
        domain::reset_for_test();
        static CHIP : DummyChip = DummyChip;

        let first = domain::register_line(HwIrq(1), &CHIP, IrqTrigger::EdgeRising)
            .expect("register first line");
        let second =
            domain::register_line(HwIrq(2), &CHIP, IrqTrigger::LevelLow).expect("register second \
                                                                                 line");
        assert_ne!(first, second, "virq must be unique");

        let resolved = domain::line(first).expect("line must resolve");
        assert_eq!(resolved.hwirq, HwIrq(1));
        assert_eq!(resolved.trigger, IrqTrigger::EdgeRising);
        assert!(resolved.chip.name() == "dummy");
        assert!(domain::line(Virq(999)).is_none(),
                "unknown virq must not resolve");

        let handle = action::request_irq(second, dummy_handler, 0xABCD).expect("request irq");
        let action = action::action(handle).expect("action must be queryable");
        assert_eq!(action.virq, second);
        assert_eq!(action.dev_id, 0xABCD);
        assert!(action::free_irq(handle),
                "free must succeed once");
        assert!(!action::free_irq(handle),
                "double free must fail");
        assert!(action::action(handle).is_none(),
                "freed action must be gone");

        assert_eq!(action::request_irq(Virq(4242), dummy_handler, 0),
                   Err(IrqError::Invalid),
                   "request on unknown virq must be rejected");
    }

    #[test]
    fn bottom_half_roundtrip() {
        domain::reset_for_test();
        bottom_half::reset_for_test();
        static BH_RAN : AtomicUsize = AtomicUsize::new(0);
        fn bh_handler(_virq : Virq, _dev_id : usize) { BH_RAN.fetch_add(1, Ordering::SeqCst); }
        static CHIP : DummyChip = DummyChip;

        let virq =
            domain::register_line(HwIrq(9), &CHIP, IrqTrigger::EdgeRising).expect("register line");
        let handle =
            action::request_irq_with_bottom_half(virq, dummy_handler, bh_handler, 0x55)
                .expect("request irq with bottom-half");
        assert_eq!(action::dispatch(virq),
                   IrqReturn::Handled,
                   "top-half must handle");
        assert!(bottom_half::has_pending(),
                "bottom-half must be scheduled");
        bottom_half::run_pending();
        assert_eq!(BH_RAN.load(Ordering::SeqCst),
                   1,
                   "bottom-half must run exactly once");
        assert!(!bottom_half::has_pending(),
                "queue must drain");
        assert!(action::free_irq(handle));
    }
}

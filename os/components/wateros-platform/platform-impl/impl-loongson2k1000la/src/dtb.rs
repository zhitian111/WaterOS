//! 引导期 DTB 物理指针占位（任务 09 从 EFI/固件 configuration table 保存）。

use core::sync::atomic::{AtomicUsize, Ordering};

static DTB_PA : AtomicUsize = AtomicUsize::new(0);

pub fn store(dtb_pa : usize) {
    DTB_PA.store(dtb_pa, Ordering::Release);
}

pub fn dtb_pa() -> usize {
    DTB_PA.load(Ordering::Acquire)
}

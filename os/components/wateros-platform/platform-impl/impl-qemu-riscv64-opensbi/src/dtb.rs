//! 引导期 DTB 物理指针（平台级 boot 状态）。
//!
//! 内存布局等平台侧解析使用该指针；设备枚举所需的 DTB 副本由驱动层另行保存。

use core::sync::atomic::{AtomicUsize, Ordering};

static DTB_PA: AtomicUsize = AtomicUsize::new(0);

/// 保存内核入口传入的 DTB 物理基址（OpenSBI `a1`）。
pub fn store(dtb_pa: usize) {
    DTB_PA.store(dtb_pa, Ordering::Release);
}

/// 当前保存的 DTB 物理基址（未保存时为 0）。
pub fn dtb_pa() -> usize {
    DTB_PA.load(Ordering::Acquire)
}

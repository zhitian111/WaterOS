//! 引导期 DTB 指针保存与早期 UART 初始化。

use crate::uart;

/// 与上层 `wateros-driver` 聚合入口的引导约定一致：保存 DTB 并初始化早期 UART。
pub fn init_when_boot(dtb_pa: usize) {
    common::dtb::store(dtb_pa);
    uart::init_early_default_uart();
}

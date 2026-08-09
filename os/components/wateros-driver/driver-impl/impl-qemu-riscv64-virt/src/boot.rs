//! 引导期 DTB 指针保存。
//!
//! DTB 物理指针由内核入口经 `a1` 传入；本模块只负责保存，供后续设备枚举使用。

/// 与上层 `wateros-driver` 聚合入口的引导约定一致：仅保存 `dtb_pa`。
pub fn init_when_boot(dtb_pa: usize) {
    common::dtb::store(dtb_pa);
}

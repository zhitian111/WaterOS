//! 内核堆尺度常量在聚合 crate 侧的再导出语义，与 `wateros-base-config::mm` 同名项保持一致。
//!
//! 平台/板级若引入不同堆布局，应同时更新本处与 `wateros-base-config` 两处定义。

#[allow(unused)]
/// 内核堆容量以 2 为底的指数位宽（字节数为 `1 << KERNEL_HEAP_SIZE_BIT_WIDTH`）。
///
/// 与 `wateros-base-config` 中同名常量应保持语义一致，供本 crate 聚合使用。
pub const KERNEL_HEAP_SIZE_BIT_WIDTH : usize = 23;

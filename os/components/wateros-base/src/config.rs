#[allow(unused)]
/// 内核堆容量以 2 为底的指数位宽（字节数为 `1 << KERNEL_HEAP_SIZE_BIT_WIDTH`）。
///
/// 与 `wateros-base-config` 中同名常量应保持语义一致，供本 crate 聚合使用。
pub const KERNEL_HEAP_SIZE_BIT_WIDTH : usize = 21;

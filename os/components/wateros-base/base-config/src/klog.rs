//! 内核消息环（`wateros-klog`）容量与单条记录上限。

/// 描述符槽数量（每条记录一个槽，槽满时覆盖最旧）。
pub const KLOG_DESC_SLOTS : usize = 256;

/// 变长正文环字节容量（当前实现按槽内缓冲聚合；保留常量供后续纯 byte-ring 扩展）。
pub const KLOG_TEXT_RING_BYTES : usize = 32 * 1024;

/// 单条记录正文最大字节数（超长截断并置 `TRUNC` 标志）。
pub const KLOG_MAX_RECORD_BYTES : usize = 1024;

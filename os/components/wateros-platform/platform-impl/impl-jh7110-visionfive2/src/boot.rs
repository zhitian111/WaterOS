//! 板级启动参数占位：真实实现从固件寄存器（a0/a1 或 mailbox）读取。

use api_v0::boot::PlatformBootArgs;

#[derive(Debug, Clone, Copy)]
/// JH7110 启动参数占位（任务 05 填充真实解析）。
pub struct JH7110BootArgs;

impl PlatformBootArgs for JH7110BootArgs {}

pub use JH7110BootArgs as BootArgs;

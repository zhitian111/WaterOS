//! 板级启动参数占位：真实实现从 PMON/UEFI 约定（a0/a1/a2）读取。

use api_v0::boot::PlatformBootArgs;

#[derive(Debug, Clone, Copy)]
/// 2K1000 启动参数占位（任务 09 填充真实解析）。
pub struct Loongson2K1000BootArgs;

impl PlatformBootArgs for Loongson2K1000BootArgs {}

pub use Loongson2K1000BootArgs as BootArgs;

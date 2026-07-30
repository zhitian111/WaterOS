//! 不绑定真实固件时使用的空启动参数。

use api_v0::boot::PlatformBootArgs;

#[derive(Debug, Clone, Copy)]
pub struct PlatformDummyBootArgs;

impl PlatformBootArgs for PlatformDummyBootArgs {}

pub use PlatformDummyBootArgs as BootArgs;

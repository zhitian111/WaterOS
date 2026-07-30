//! 不绑定真实固件时使用的空启动参数。

use api_v0::boot::PlatformBootArgs;

#[derive(Debug, Clone, Copy)]
pub struct PlatformDummyBootArgs;

#[derive(Debug, Clone, Copy)]
pub struct PlatformDummyBootContext;

impl PlatformBootArgs for PlatformDummyBootArgs {}

impl From<PlatformDummyBootArgs> for PlatformDummyBootContext {
    #[inline]
    fn from(_ : PlatformDummyBootArgs) -> Self { Self }
}

pub use PlatformDummyBootArgs as BootArgs;
pub use PlatformDummyBootContext as BootContext;

//! 不绑定真实固件时使用的空启动参数。

use api_v0::boot::PlatformBootArgs;

#[derive(Debug, Clone, Copy)]
pub struct PlatformDummyBootArgs;

impl PlatformBootArgs for PlatformDummyBootArgs {}

pub unsafe fn init_command_line(_arg0: usize, _arg1: usize, _arg2: usize) {}

pub fn command_line() -> Option<&'static str> { None }

pub use PlatformDummyBootArgs as BootArgs;

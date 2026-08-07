#![no_std]
//! WaterOS 终端聚合层。
//!
//! 调用方通过本 crate 访问终端策略和处理后的输入。UART 设备发现仍由 driver/VFS
//! 适配层负责，Linux ioctl 数据编解码仍由 syscall 层负责。

/// 与具体实现无关的版本化终端类型和常量。
#[cfg(feature = "api-v0")]
pub mod api {
    pub use api_v0::*;
}

#[cfg(feature = "api-v0")]
pub use api_v0::*;
#[cfg(feature = "impl-console")]
pub use impl_console::*;

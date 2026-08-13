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

/// 终端内核态可用性自检；测试结束后恢复默认终端状态。
#[cfg(feature = "self_test")]
pub fn self_test() {
    let before = termios();
    let transformed = transform_output(b"tty\n");
    assert_eq!(transformed, b"tty\r\n");
    configure(api_v0::ConsoleTtyMode::Interactive);
    set_termios(before, true);
    log::info!("[tty] self_test complete; terminal state restored");
}

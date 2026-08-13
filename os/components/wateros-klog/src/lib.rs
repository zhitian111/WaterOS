#![no_std]
//! klog 聚合门面：导出版本化 API、当前内核实现和稳定日志宏。
//!
//! 所有状态和 `syslog` 实现均位于 `klog-impl/*`；本 crate 不保存全局数据。

/// 版本化 klog API 契约。
pub mod api {
    pub use ::api_v0::*;
}

/// 当前 klog 内核实现命名空间。
pub use impl_kernel as active_impl;

/// 保持既有 `klog::syscall::dispatch_kernel` 路径的兼容转发模块。
pub mod syscall {
    pub use crate::active_impl::dispatch_kernel;
}

pub use active_impl::*;
pub use api_v0::*;

#[cfg(feature = "self_test")]
pub fn self_test() {
    log::info!("[klog] self_test begin");
    active_impl::self_test();
    log::info!("[klog] self_test complete");
}

#[macro_export]
macro_rules! klog_trace {
    ($($arg:tt)*) => {{
        let _ = $crate::record_fmt($crate::api::LOG_DEBUG, core::format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! klog_debug {
    ($($arg:tt)*) => {{
        let _ = $crate::record_fmt($crate::api::LOG_DEBUG, core::format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! klog_info {
    ($($arg:tt)*) => {{
        let _ = $crate::record_fmt($crate::api::LOG_INFO, core::format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! klog_warn {
    ($($arg:tt)*) => {{
        let _ = $crate::record_fmt($crate::api::LOG_WARNING, core::format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! klog_error {
    ($($arg:tt)*) => {{
        let _ = $crate::record_fmt($crate::api::LOG_ERR, core::format_args!($($arg)*));
    }};
}

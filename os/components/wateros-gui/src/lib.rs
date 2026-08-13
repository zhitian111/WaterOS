//! WaterOS GUI 聚合入口。
//!
//! 上层只依赖本 crate；`api` 提供稳定数据模型，默认 `impl-software` 提供基于
//! `driver-display` framebuffer 的窗口、控件、事件和双缓冲实现。

#![no_std]

#[cfg(feature = "api-v0")]
pub mod api {
    pub use api_v0::*;
}

#[cfg(feature = "api-v0")]
pub use api_v0::*;
#[cfg(feature = "impl-software")]
pub use impl_software::*;

#[cfg(all(feature = "self_test", feature = "impl-software"))]
pub fn self_test() {
    impl_software::self_test();
    log::info!("[gui] self_test complete; temporary software surface reclaimed");
}

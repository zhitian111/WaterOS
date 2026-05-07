//! 虚拟文件系统 **聚合 crate**：对上重导出 [`api_v0`]  trait 与类型，对下挂载占位或真实桥接实现。
//!
//! - [`api`]：单根只读视图 [`api_v0::SingleRootReadView`]、可写会话 [`api_v0::RootRwSession`] 及错误/元数据等 **语义契约**。
//! - [`dummy`]：无块设备时的占位根视图，路径规范化仍可用，卷访问返回未挂载。
//! - [`bridge`]（feature `bridge-fs-api`）：通过 `wateros-fs` 将根卷与 devfs 接到上述 trait，供内核启动与自检使用。
//!
//! 路径形状统一由 `api-v0` 的 [`api_v0::normalize_absolute_path`] 处理；**不** 在此 crate 内实现具体文件系统格式。

#![no_std]

pub mod api {
    pub use ::api_v0::*;
}

pub use api_v0::*;

pub mod dummy {
    pub use ::impl_dummy::*;
}

#[cfg(feature = "bridge-fs-api")]
pub mod bridge {
    pub use ::impl_fs_bridge::*;
}

pub fn test() {
    api_v0::test();
    impl_dummy::test();
    #[cfg(feature = "bridge-fs-api")]
    impl_fs_bridge::test();
}

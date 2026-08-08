//! 当前 platform profile 暴露的原始启动参数。
//!
//! BOOT_CONTRACT: 本模块只转发 profile 对固件寄存器的解释；它不初始化 CPU-local、
//! trap、页表或 scheduler。WaterOS 不把 QEMU bootargs 当作运行时配置源。

pub use crate::active_impl::boot::BootArgs;
pub use api_v0::boot::PlatformBootArgs;

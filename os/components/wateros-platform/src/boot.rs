//! 当前 platform profile 暴露的启动参数和启动上下文。
//!
//! BOOT_CONTRACT: 本模块只转发 profile 对固件寄存器的解释；它不初始化 CPU-local、
//! trap、页表或 scheduler。BSP 与 AP 必须在使用堆和 scheduler 前完成各自初始化。

pub use crate::active_impl::boot::{BootArgs, BootContext};
pub use api_v0::boot::PlatformBootArgs;

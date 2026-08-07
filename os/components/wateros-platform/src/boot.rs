//! 当前 platform profile 暴露的原始启动参数。
//!
//! BOOT_CONTRACT: 本模块只转发 profile 对固件寄存器的解释；它不初始化 CPU-local、
//! trap、页表或 scheduler。BSP 与 AP 必须在使用堆和 scheduler 前完成各自初始化。

pub use crate::active_impl::boot::BootArgs;
pub use api_v0::boot::PlatformBootArgs;

/// Initialize the selected platform's persistent command-line view. The raw
/// arguments retain their board-specific meaning.
///
/// # Safety
/// The pointer-valued arguments must be the original firmware values and must
/// remain readable for the duration required by the selected platform parser.
pub unsafe fn init_command_line(arg0: usize, arg1: usize, arg2: usize) {
    unsafe { crate::active_impl::boot::init_command_line(arg0, arg1, arg2) }
}

/// Return the normalized kernel command line saved by the BSP.
pub fn command_line() -> Option<&'static str> {
    crate::active_impl::boot::command_line()
}

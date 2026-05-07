#![no_std]
//! 用户态与内核共享的 ABI 定义聚合层。
//!
//! 按 feature 选择具体 API 版本与平台实现（如 Linux/riscv64 系统调用号表），
//! 供 syscall 分发与用户态返回值编码等模块统一引用。
#[cfg(feature = "api-v0")]
pub mod user_ret {
    pub use api_v0::user_ret::*;
}
#[cfg(feature = "api-v0")]
pub mod errno {
    pub use api_v0::errno::*;
}
#[cfg(feature = "api-v0")]
pub mod syscall_number {
    pub use api_v0::syscall_number::{SyscallNumber, SyscallNumberTable};
    #[cfg(feature = "impl-linux-riscv64")]
    pub use impl_linux_riscv64::LinuxRiscv64 as ActiveSyscallNumberTable;
}
#[cfg(feature = "api-v0")]
pub mod syscall_args {
    pub use api_v0::syscall_args::*;
}

#![no_std]
//! 用户态与内核共享的 ABI 聚合入口。
//!
//! 按 feature 挂上具体 API 版本与平台实现（例如 Linux 通用 64 位调用号表），
//! syscall 分发与用户返回值编码等模块统一从这里引用。

/// 用户态可见的系统调用返回值编码（`api-v0` 重导出）。
#[cfg(feature = "api-v0")]
pub mod user_ret {
    pub use api_v0::user_ret::*;
}

/// Linux 风格错误码与 `-errno` 用户态约定（`api-v0` 重导出）。
#[cfg(feature = "api-v0")]
pub mod errno {
    pub use api_v0::errno::*;
}

/// 系统调用编号类型。
#[cfg(feature = "api-v0")]
pub mod syscall_number {
    pub use api_v0::syscall_number::SyscallNumber;
}

/// 陷阱帧/寄存器快照与内核分发层之间的 C 布局参数包（`api-v0` 重导出）。
#[cfg(feature = "api-v0")]
pub mod syscall_args {
    pub use api_v0::syscall_args::*;
}

#![no_std]
//! 用户态与内核共享的 ABI 定义聚合层。
//!
//! 按 feature 选择具体 API 版本与平台实现（如 Linux/riscv64 系统调用号表），
//! 供 syscall 分发与用户态返回值编码等模块统一引用。
//!
//! English: workspace-facing re-exports of syscall numbers, errno, packed args, and
//! user return encoding; concrete tables are selected by `impl-*` features.

/// 用户态可见的系统调用返回值编码（`api-v0` 重导出）。
///
/// English: user-visible syscall return encoding (re-export from `api-v0`).
#[cfg(feature = "api-v0")]
pub mod user_ret {
    pub use api_v0::user_ret::*;
}

/// Linux 风格错误码与 `-errno` 用户态约定（`api-v0` 重导出）。
///
/// English: Linux-style errno values and user-visible negative returns (`api-v0`).
#[cfg(feature = "api-v0")]
pub mod errno {
    pub use api_v0::errno::*;
}

/// 系统调用编号类型与「号 → 常量」表 trait；可选附带当前选中的 Linux 通用 64 位号表别名。
///
/// English: syscall number newtype, `SyscallNumberTable` trait, and optional
/// `ActiveSyscallNumberTable` alias when `impl-linux-generic64` is enabled.
#[cfg(feature = "api-v0")]
pub mod syscall_number {
    pub use api_v0::syscall_number::{SyscallNumber, SyscallNumberTable};
    /// 启用 `impl-linux-generic64` 时，与 asm-generic 64 位表绑定的活动号表类型别名。
    ///
    /// English: active syscall number table type alias backed by the Linux generic
    /// 64-bit table when feature `impl-linux-generic64` is on.
    #[cfg(feature = "impl-linux-generic64")]
    pub use impl_linux_generic64::LinuxGeneric64 as ActiveSyscallNumberTable;
}

/// 陷阱帧/寄存器快照与内核分发层之间的 C 布局参数包（`api-v0` 重导出）。
///
/// English: `repr(C)` syscall argument carriers between trap frames and the kernel
/// dispatcher (`api-v0` re-export).
#[cfg(feature = "api-v0")]
pub mod syscall_args {
    pub use api_v0::syscall_args::*;
}

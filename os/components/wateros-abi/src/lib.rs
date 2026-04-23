#![no_std]
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

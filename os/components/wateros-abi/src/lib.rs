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
    #[cfg(feature = "impl-linux-generic64")]
    pub use impl_linux_generic64::LinuxGeneric64 as ActiveSyscallNumberTable;
}
#[cfg(feature = "api-v0")]
pub mod syscall_args {
    pub use api_v0::syscall_args::*;
}

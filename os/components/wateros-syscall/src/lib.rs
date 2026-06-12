#![no_std]
//! WaterOS syscall aggregate crate.
//!
//! `api` exposes the v0 trap-facing contract, while `active_impl` points at the
//! kernel dispatcher selected by feature flags.

#[cfg(feature = "api-v0")]
pub mod api {
    pub use api_v0::*;
}

#[cfg(feature = "impl-kernel")]
pub use impl_kernel as active_impl;

#[cfg(feature = "api-v0")]
pub use api_v0::SyscallDispatcher;

#[cfg(feature = "impl-kernel")]
use abi::syscall_args::SyscallArgs;

/// Trap / exception return path syscall dispatch entry.
#[cfg(feature = "impl-kernel")]
#[inline]
pub fn dispatch_syscall_from_trap(syscall_nr : usize, syscall_args : SyscallArgs) -> isize {
    active_impl::dispatch_syscall_from_trap(syscall_nr, syscall_args)
}

/// Current-task syscall dispatch entry for assembly or C ABI callers.
#[cfg(feature = "impl-kernel")]
#[unsafe(no_mangle)]
pub extern "C" fn __wateros_syscall_dispatch_current(syscall_nr : usize,
                                                     arg0 : usize,
                                                     arg1 : usize,
                                                     arg2 : usize,
                                                     arg3 : usize,
                                                     arg4 : usize,
                                                     arg5 : usize)
                                                     -> isize {
    let syscall_args = SyscallArgs::from_regs([arg0, arg1, arg2, arg3, arg4, arg5]);
    dispatch_syscall_from_trap(syscall_nr, syscall_args)
}

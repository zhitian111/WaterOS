#![no_std]
//! WaterOS syscall v0 的公共边界。
//!
//! 本 crate 同时定义 Linux generic 64 位调用号，以及 trap、内核分发器共享的
//! 参数、错误和返回值类型。它不依赖 platform、task 或 syscall 内核实现。

pub mod args;
pub mod errno;
pub mod number;
pub mod return_value;

pub use args::{SyscallArgs, SyscallPacket};
pub use errno::{ErrNo, KernelResult};
pub use number::*;
pub use return_value::UserRet;

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::{ErrNo, KernelResult, SyscallArgs, UserRet};

    #[test]
    fn errno_is_positive_until_user_return_encoding() {
        assert_eq!(ErrNo::from_raw(22), Some(ErrNo::EINVAL));
        assert_eq!(ErrNo::from_raw(0), None);
        assert_eq!(ErrNo::from_raw(-22), None);
        assert_eq!(ErrNo::EINVAL.raw(), 22);
        assert_eq!(ErrNo::EINVAL.user_ret(), -22);
    }

    #[test]
    fn user_return_encodes_kernel_result_once() {
        let ok : KernelResult<usize> = Ok(7);
        let error : KernelResult<usize> = Err(ErrNo::EINVAL);
        assert_eq!(UserRet::from_kernel_result(ok).0, 7);
        assert_eq!(UserRet::from_kernel_result(error).0, -22);
    }

    #[test]
    fn syscall_args_preserve_register_slot_order() {
        let regs = [0; config::syscall::MAX_SYSCALL_ARGS];
        let args = SyscallArgs::from_regs(regs);
        assert_eq!(args.arg(0), 0);
        assert_eq!(args.as_regs(), regs);
    }
}

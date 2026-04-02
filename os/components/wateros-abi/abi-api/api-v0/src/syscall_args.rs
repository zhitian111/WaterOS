#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SyscallArgs {
    args : [usize; config::syscall::MAX_SYSCALL_ARGS],
}

impl SyscallArgs {
    #[inline]
    #[allow(unused)]
    pub const fn from_regs(regs : [usize; MAX_SYSCALL_ARGS]) -> Self { Self { args : regs } }
    #[inline]
    #[allow(unused)]
    pub const fn arg(&self, idx : usize) -> usize { self.args[idx] }
    #[inline]
    #[allow(unused)]
    pub const fn as_regs(&self) -> [usize; config::syscall::MAX_SYSCALL_ARGS] { self.args }
}

use config::syscall::MAX_SYSCALL_ARGS;

use crate::syscall_number::SyscallNumber;

#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct SyscallPacket {
    pub nr : SyscallNumber,
    pub args : SyscallArgs,
}

impl SyscallPacket {
    #[inline]
    #[allow(unused)]
    pub const fn new(nr : SyscallNumber, args : SyscallArgs) -> Self {
        Self { nr : nr,
               args : args }
    }
}

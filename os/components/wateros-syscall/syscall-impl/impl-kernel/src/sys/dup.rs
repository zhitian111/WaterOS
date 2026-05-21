//! `dup`/`dup3` 系统调用实现（stub）。
//!
//! 当前 `VfsIoHandle` 尚无 `duplicate` 方法，暂返回 `ENOSYS`。
//! 待 `VfsIoHandle` 增加 `duplicate` trait 方法后实现。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

/// `dup(oldfd)` — 复制 fd 到最低可用编号（stub）。
pub(crate) fn sys_dup(_args : SyscallArgs) -> UserRet { UserRet::from_error(ErrNo::ENOSYS) }

/// `dup3(oldfd, newfd, flags)` — 复制 fd 到指定编号（stub）。
pub(crate) fn sys_dup3(_args : SyscallArgs) -> UserRet { UserRet::from_error(ErrNo::ENOSYS) }

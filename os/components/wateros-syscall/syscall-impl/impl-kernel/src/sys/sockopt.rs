//! `setsockopt(2)` / `getsockopt(2)` — 极简存根。

use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

pub(crate) fn sys_setsockopt(args: SyscallArgs) -> UserRet {
    let _fd = args.arg(0);
    let _level = args.arg(1);
    let _optname = args.arg(2);
    let _optval = args.arg(3);
    let _optlen = args.arg(4);
    // 极简实现：所有选项默认成功
    UserRet::from_success(0)
}

pub(crate) fn sys_getsockopt(args: SyscallArgs) -> UserRet {
    let _fd = args.arg(0);
    let _level = args.arg(1);
    let _optname = args.arg(2);
    let _optval = args.arg(3);
    let _optlen = args.arg(4);
    // 极简实现：默认成功
    UserRet::from_success(0)
}

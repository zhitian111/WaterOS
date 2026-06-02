//! `ioctl(2)` minimal TTY compatibility used by BusyBox probes.

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;

use crate::user_copy::copy_to_user_struct;

const TCGETS: usize = 0x5401;
const TIOCGWINSZ: usize = 0x5413;

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxWinSize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

pub(crate) fn sys_ioctl(args: SyscallArgs) -> UserRet {
    let _fd = args.arg(0);
    let request = args.arg(1);
    let argp = args.arg(2);

    match request {
        TCGETS => UserRet::from_error(ErrNo::ENOTTY),
        TIOCGWINSZ => {
            if argp == 0 {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            let winsize = LinuxWinSize {
                ws_row: 25,
                ws_col: 80,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            match copy_to_user_struct(argp, &winsize) {
                Ok(()) => UserRet::from_success(0),
                Err(e) => UserRet::from_error(e),
            }
        }
        _ => UserRet::from_error(ErrNo::ENOTTY),
    }
}

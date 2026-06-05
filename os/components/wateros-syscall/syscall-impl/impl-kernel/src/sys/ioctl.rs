//! `ioctl(2)`：优先按 fd 句柄分发；RTC 与 TTY 兼容 fallback。

use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::api::VfsError;

use crate::sys::rtc::sys_rtc_ioctl;
use crate::user_copy::copy_to_user_struct;
use crate::vfs_util::vfs_error_to_errno;

const TCGETS: u32 = 0x5401;
const TIOCGWINSZ: u32 = 0x5413;
const RTC_RD_TIME: u32 = 0x8024_7009;
const RTC_SET_TIME: u32 = 0x4024_700a;

fn ioctl_req(raw: usize) -> u32 {
    raw as u32
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxWinSize {
    ws_row: u16,
    ws_col: u16,
    ws_xpixel: u16,
    ws_ypixel: u16,
}

pub(crate) fn sys_ioctl(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let request = ioctl_req(args.arg(1));
    let argp = args.arg(2);

    if vfs::fd::current_fd_is_rtc(fd).unwrap_or(false) {
        return sys_rtc_ioctl(request, argp);
    }

    match vfs::fd::with_current_io(fd, |handle| handle.ioctl(request as usize, argp)) {
        Ok(v) => UserRet::from_success(v as usize),
        Err(VfsError::Unsupported) => {
            if matches!(request, RTC_RD_TIME | RTC_SET_TIME) {
                return sys_rtc_ioctl(request, argp);
            }
            global_ioctl_fallback(request, argp)
        }
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

fn global_ioctl_fallback(request: u32, argp: usize) -> UserRet {
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

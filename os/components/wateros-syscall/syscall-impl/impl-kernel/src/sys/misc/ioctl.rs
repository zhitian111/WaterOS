//! `ioctl(2)`：优先按 fd 句柄分发；RTC 与 TTY 兼容 fallback。

//! 本模块代码由AI完成
use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::api::VfsError;

use crate::sys::time::rtc::sys_rtc_ioctl;
use crate::user_copy::copy_to_user_struct;
use crate::vfs_util::vfs_error_to_errno;

const TCGETS: u32 = 0x5401;
const TCSETS: u32 = 0x5402;
const TCSETSW: u32 = 0x5403;
const TCSETSF: u32 = 0x5404;
const TIOCGPGRP: u32 = 0x540f;
const TIOCGWINSZ: u32 = 0x5413;
const FIONREAD: u32 = 0x541b;
const TIOCNOTTY: u32 = 0x5422;
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

fn ioctl_enotty(request: u32, fd: Option<usize>, argp: usize) -> UserRet {
    match fd {
        Some(fd) => log::warn!(
            "[syscall] ioctl(nr=29) unsupported request={:#x} fd={fd} argp={argp:#x}",
            request,
        ),
        None => log::warn!(
            "[syscall] ioctl(nr=29) unsupported request={:#x} argp={argp:#x}",
            request,
        ),
    }
    UserRet::from_error(ErrNo::ENOTTY)
}

fn tty_char_ioctl(request: u32, argp: usize) -> UserRet {
    match request {
        // glibc tcgetattr 常以 a2=sp 调用 TCGETS，成功写 termios 会覆盖同帧内的栈金丝雀。
        TCGETS => UserRet::from_error(ErrNo::ENOTTY),
        // 终端属性状态尚未建模；允许 stty/read -s 等保存/恢复路径继续执行。
        TCSETS | TCSETSW | TCSETSF => {
            if argp == 0 {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            UserRet::from_success(0)
        }
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
        TIOCGPGRP => {
            if argp == 0 {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            let pgrp = task::current_task_id().unwrap_or(0) as i32;
            match copy_to_user_struct(argp, &pgrp) {
                Ok(()) => UserRet::from_success(0),
                Err(e) => UserRet::from_error(e),
            }
        }
        // 守护进程化常用此 ioctl 脱离控制终端；WaterOS 尚未建模控制终端，按 no-op 处理。
        TIOCNOTTY => UserRet::from_success(0),
        _ => ioctl_enotty(request, None, argp),
    }
}

// 本方法代码由AI完成
pub(crate) fn sys_ioctl(args: SyscallArgs) -> UserRet {
    let fd = args.arg(0);
    let request = ioctl_req(args.arg(1));
    let argp = args.arg(2);

    if vfs::fd::current_fd_is_rtc(fd).unwrap_or(false) {
        return sys_rtc_ioctl(request, argp);
    }

    if vfs::fd::current_fd_is_tty_char(fd).unwrap_or(false) {
        return tty_char_ioctl(request, argp);
    }

    if request == FIONREAD {
        return pipe_fionread(fd, argp);
    }

    match vfs::fd::with_current_io(fd, |handle| handle.ioctl(request as usize, argp)) {
        Ok(v) => UserRet::from_success(v as usize),
        Err(VfsError::Unsupported) => {
            if matches!(request, RTC_RD_TIME | RTC_SET_TIME) {
                return sys_rtc_ioctl(request, argp);
            }
            global_ioctl_fallback(fd, request, argp)
        }
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

fn pipe_fionread(fd: usize, argp: usize) -> UserRet {
    if argp == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    match vfs::fd::with_current_io(fd, |handle| Ok(handle.pipe_buffer_len())) {
        Ok(Some(len)) => {
            let available = len.min(i32::MAX as usize) as i32;
            match copy_to_user_struct(argp, &available) {
                Ok(()) => UserRet::from_success(0),
                Err(e) => UserRet::from_error(e),
            }
        }
        Ok(None) => ioctl_enotty(FIONREAD, Some(fd), argp),
        Err(e) => UserRet::from_error(vfs_error_to_errno(e)),
    }
}

fn global_ioctl_fallback(fd: usize, request: u32, argp: usize) -> UserRet {
    match request {
        TCGETS => ioctl_enotty(request, Some(fd), argp),
        TCSETS | TCSETSW | TCSETSF => tty_char_ioctl(request, argp),
        TIOCGWINSZ => tty_char_ioctl(TIOCGWINSZ, argp),
        TIOCNOTTY => UserRet::from_success(0),
        _ => ioctl_enotty(request, Some(fd), argp),
    }
}

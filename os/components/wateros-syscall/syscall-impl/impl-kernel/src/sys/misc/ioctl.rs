//! `ioctl(2)`：优先按 fd 句柄分发；RTC 与 TTY 兼容 fallback。

//! 本模块代码由AI完成
use abi::errno::ErrNo;
use abi::syscall_args::SyscallArgs;
use abi::user_ret::UserRet;
use vfs::api::VfsError;

use crate::sys::time::rtc::sys_rtc_ioctl;
use crate::user_copy::{copy_from_user_struct, copy_to_user_struct};
use crate::vfs_util::vfs_error_to_errno;

const TCGETS: u32 = 0x5401;
const TCSETS: u32 = 0x5402;
const TCSETSW: u32 = 0x5403;
const TCSETSF: u32 = 0x5404;
const TIOCGPGRP: u32 = 0x540f;
const TIOCGWINSZ: u32 = 0x5413;
const FIONREAD: u32 = 0x541b;
const FIONBIO: u32 = 0x5421;
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

const NCCS: usize = 19;

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxTermios {
    c_iflag: u32,
    c_oflag: u32,
    c_cflag: u32,
    c_lflag: u32,
    c_line: u8,
    c_cc: [u8; NCCS],
}

const DEFAULT_TERMIOS: LinuxTermios = LinuxTermios {
    c_iflag: 0x500,
    c_oflag: 0x5,
    c_cflag: 0xbf,
    c_lflag: 0x8a3b,
    c_line: 0,
    c_cc: [
        3, 28, 127, 21, 4, 0, 1, 0, 17, 19, 26, 0, 18, 15, 23, 22, 0, 0, 0,
    ],
};

static TTY_TERMIOS: spin::Mutex<LinuxTermios> = spin::Mutex::new(DEFAULT_TERMIOS);

fn ioctl_enotty(request: u32, fd: Option<usize>, argp: usize) -> UserRet {
    match fd {
        Some(fd) => log::trace!(
            "[syscall] ioctl(nr=29) unsupported request={:#x} fd={fd} argp={argp:#x}",
            request,
        ),
        None => log::trace!(
            "[syscall] ioctl(nr=29) unsupported request={:#x} argp={argp:#x}",
            request,
        ),
    }
    UserRet::from_error(ErrNo::ENOTTY)
}

fn tty_char_ioctl(request: u32, argp: usize) -> UserRet {
    match request {
        TCGETS => {
            if argp == 0 {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            let termios = *TTY_TERMIOS.lock();
            match copy_to_user_struct(argp, &termios) {
                Ok(()) => UserRet::from_success(0),
                Err(e) => UserRet::from_error(e),
            }
        }
        TCSETS | TCSETSW | TCSETSF => {
            if argp == 0 {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            let termios = match copy_from_user_struct::<LinuxTermios>(argp) {
                Ok(termios) => termios,
                Err(error) => return UserRet::from_error(error),
            };
            *TTY_TERMIOS.lock() = termios;
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
    if request == FIONBIO {
        return fd_fionbio(fd, argp);
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

fn fd_fionbio(fd : usize, argp : usize) -> UserRet {
    const O_NONBLOCK : usize = 0o4000;
    if argp == 0 {
        return UserRet::from_error(ErrNo::EFAULT);
    }
    let enabled = match copy_from_user_struct::<i32>(argp) {
        Ok(value) => value != 0,
        Err(error) => return UserRet::from_error(error),
    };

    if let Some(mut flags) = crate::socket_fd::status_flags(fd) {
        if enabled {
            flags |= O_NONBLOCK;
        } else {
            flags &= !O_NONBLOCK;
        }
        return match crate::socket_fd::set_status_flags(fd, flags) {
            Some(()) => UserRet::from_success(0),
            None => UserRet::from_error(ErrNo::EBADF),
        };
    }

    match vfs::fd::with_current_io(fd, |handle| {
        let mut flags = handle.open_status_flags();
        if enabled {
            flags |= O_NONBLOCK as u32;
        } else {
            flags &= !(O_NONBLOCK as u32);
        }
        handle.set_open_status_flags(flags)
    }) {
        Ok(()) => UserRet::from_success(0),
        Err(error) => UserRet::from_error(vfs_error_to_errno(error)),
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
    ioctl_enotty(request, Some(fd), argp)
}

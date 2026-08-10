//! `ioctl(2)`：优先按 fd 句柄分发；RTC 与 TTY 兼容 fallback。

//! 本模块代码由AI完成
use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use vfs::api::VfsError;

use crate::sys::time::rtc::sys_rtc_ioctl;
use crate::user_copy::{copy_from_user_struct, copy_to_user, copy_to_user_struct};
use crate::vfs_util::vfs_error_to_errno;

const TCGETS: u32 = 0x5401;
const TCSETS: u32 = 0x5402;
const TCSETSW: u32 = 0x5403;
const TCSETSF: u32 = 0x5404;
const TIOCSCTTY: u32 = 0x540e;
const TIOCGPGRP: u32 = 0x540f;
const TIOCSPGRP: u32 = 0x5410;
const TIOCGWINSZ: u32 = 0x5413;
const TIOCSWINSZ: u32 = 0x5414;
const FIONREAD: u32 = 0x541b;
const FIONBIO: u32 = 0x5421;
const TIOCNOTTY: u32 = 0x5422;
const TIOCGSID: u32 = 0x5429;
const RTC_RD_TIME: u32 = 0x8024_7009;
const RTC_SET_TIME: u32 = 0x4024_700a;
const EVIOCGVERSION: u32 = 0x8004_4501;
const EVIOCGID: u32 = 0x8008_4502;

fn evdev_ioc(dir: u32, size: usize, nr: u32) -> u32 {
    (dir << 30) | ((size as u32 & 0x3fff) << 16) | (0x45 << 8) | (nr & 0xff)
}

fn evdev_query_ioctl(request: u32, argp: usize, index: usize) -> UserRet {
    if argp == 0 { return UserRet::from_error(ErrNo::EFAULT); }
    let Ok(info) = driver_input::input_device_info(index) else {
        return UserRet::from_error(ErrNo::EINVAL);
    };
    let size = ((request >> 16) & 0x3fff) as usize;
    let mut output = [0u8; 64];
    let output_len = if request == EVIOCGVERSION {
        if size != 4 { return UserRet::from_error(ErrNo::EINVAL); }
        output[..4].copy_from_slice(&0x0001_0001u32.to_le_bytes()); 4
    } else if request == EVIOCGID {
        if size != 8 { return UserRet::from_error(ErrNo::EINVAL); }
        let fields = [info.id.bustype, info.id.vendor, info.id.product, info.id.version];
        for (offset, field) in fields.into_iter().enumerate() { output[offset * 2..offset * 2 + 2].copy_from_slice(&field.to_le_bytes()); }
        8
    } else if (request & 0xff00) == (0x45 << 8) && (request & 0xff) == 0x06 {
        if size == 0 { return UserRet::from_error(ErrNo::EINVAL); }
        let name = info.name.as_bytes();
        let len = name.len().min(size.saturating_sub(1)).min(output.len() - 1);
        output[..len].copy_from_slice(&name[..len]); output[len] = 0; len + 1
    } else if (request & 0xff00) == (0x45 << 8) && (request & 0xff) >= 0x20 {
        let event = (request & 0xff) - 0x20;
        let bits : &[u8] = match event { 0 => &info.event_types.to_le_bytes(), 1 => &info.key_bits, 2 => &info.relative_bits, 3 => &info.absolute_bits, _ => return UserRet::from_error(ErrNo::ENOTTY) };
        let len = size.min(bits.len()).min(output.len()); output[..len].copy_from_slice(&bits[..len]); len
    } else { return UserRet::from_error(ErrNo::ENOTTY); };
    match copy_to_user(argp, &output[..output_len]) { Ok(n) if n == output_len => UserRet::from_success(0), Ok(_) => UserRet::from_error(ErrNo::EFAULT), Err(e) => UserRet::from_error(e) }
}

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

const NCCS: usize = tty::NCCS;

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

impl From<tty::TtyTermios> for LinuxTermios {
    fn from(value: tty::TtyTermios) -> Self {
        Self { c_iflag: value.iflag,
               c_oflag: value.oflag,
               c_cflag: value.cflag,
               c_lflag: value.lflag,
               c_line: value.line,
               c_cc: value.cc }
    }
}

impl From<LinuxTermios> for tty::TtyTermios {
    fn from(value: LinuxTermios) -> Self {
        Self { iflag: value.c_iflag,
               oflag: value.c_oflag,
               cflag: value.c_cflag,
               lflag: value.c_lflag,
               line: value.c_line,
               cc: value.c_cc }
    }
}

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
            let termios = LinuxTermios::from(tty::termios());
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
            tty::set_termios(termios.into(), request == TCSETSF);
            UserRet::from_success(0)
        }
        TIOCGWINSZ => {
            if argp == 0 {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            let current = tty::winsize();
            let winsize = LinuxWinSize { ws_row: current.row,
                                         ws_col: current.col,
                                         ws_xpixel: current.xpixel,
                                         ws_ypixel: current.ypixel };
            match copy_to_user_struct(argp, &winsize) {
                Ok(()) => UserRet::from_success(0),
                Err(e) => UserRet::from_error(e),
            }
        }
        TIOCGPGRP => {
            if argp == 0 {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            let pgrp = tty::foreground_pgid().min(i32::MAX as usize) as i32;
            match copy_to_user_struct(argp, &pgrp) {
                Ok(()) => UserRet::from_success(0),
                Err(e) => UserRet::from_error(e),
            }
        }
        TIOCSPGRP => {
            if argp == 0 {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            let pgrp = match copy_from_user_struct::<i32>(argp) {
                Ok(value) if value > 0 => value as usize,
                Ok(_) => return UserRet::from_error(ErrNo::EINVAL),
                Err(error) => return UserRet::from_error(error),
            };
            if !task::pgid_has_members(task::ProcessId::from_raw(pgrp)) {
                return UserRet::from_error(ErrNo::ESRCH);
            }
            let Some(caller) = task::current_process_snapshot() else {
                return UserRet::from_error(ErrNo::ESRCH);
            };
            let same_session = task::process_pids_in_pgid(task::ProcessId::from_raw(pgrp))
                .into_iter()
                .filter_map(task::process_snapshot)
                .any(|member| member.sid == caller.sid);
            if !same_session {
                return UserRet::from_error(ErrNo::EPERM);
            }
            tty::set_foreground_pgid(pgrp);
            UserRet::from_success(0)
        }
        TIOCSCTTY => {
            let Some(process) = task::current_process_snapshot() else {
                return UserRet::from_error(ErrNo::ESRCH);
            };
            let sid = if process.sid.raw() == 0 { process.pid.raw() } else { process.sid.raw() };
            let controlling = tty::controlling_sid();
            if controlling != 0 && controlling != sid && argp == 0 {
                return UserRet::from_error(ErrNo::EPERM);
            }
            tty::set_controlling_sid(sid);
            tty::set_foreground_pgid(process.pgid.raw());
            UserRet::from_success(0)
        }
        TIOCNOTTY => {
            let Some(process) = task::current_process_snapshot() else {
                return UserRet::from_error(ErrNo::ESRCH);
            };
            if tty::controlling_sid() != 0 &&
               tty::controlling_sid() != process.sid.raw() &&
               tty::controlling_sid() != process.pid.raw()
            {
                return UserRet::from_error(ErrNo::ENOTTY);
            }
            tty::detach_controlling_terminal();
            UserRet::from_success(0)
        }
        TIOCSWINSZ => {
            if argp == 0 {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            let winsize = match copy_from_user_struct::<LinuxWinSize>(argp) {
                Ok(value) => value,
                Err(error) => return UserRet::from_error(error),
            };
            tty::set_winsize(tty::TtyWinSize { row: winsize.ws_row,
                                               col: winsize.ws_col,
                                               xpixel: winsize.ws_xpixel,
                                               ypixel: winsize.ws_ypixel });
            let foreground = tty::foreground_pgid();
            if foreground != 0 {
                crate::sys::ipc::signal::send_kernel_signal_to_process_group(
                    task::ProcessId::from_raw(foreground),
                    ipc::signal::SIGWINCH,
                );
            }
            UserRet::from_success(0)
        }
        TIOCGSID => {
            if argp == 0 {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            let sid = tty::controlling_sid().min(i32::MAX as usize) as i32;
            match copy_to_user_struct(argp, &sid) {
                Ok(()) => UserRet::from_success(0),
                Err(error) => UserRet::from_error(error),
            }
        }
        FIONREAD => {
            if argp == 0 {
                return UserRet::from_error(ErrNo::EFAULT);
            }
            let available = tty::readable_len().min(i32::MAX as usize) as i32;
            match copy_to_user_struct(argp, &available) {
                Ok(()) => UserRet::from_success(0),
                Err(error) => UserRet::from_error(error),
            }
        }
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

    if request == FIONBIO {
        return fd_fionbio(fd, argp);
    }

    if vfs::fd::current_fd_is_tty_char(fd).unwrap_or(false) {
        return tty_char_ioctl(request, argp);
    }

    if let Ok(Some(index)) = vfs::fd::current_fd_input_event_index(fd) {
        return evdev_query_ioctl(request, argp, index);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evdev_requests_use_linux_ioc_encoding() {
        assert_eq!(evdev_ioc(2, 4, 1), EVIOCGVERSION);
        assert_eq!(evdev_ioc(2, 8, 2), EVIOCGID);
        assert_eq!(evdev_ioc(2, 32, 0x20), 0x8020_4520);
        assert_eq!(((evdev_ioc(2, 17, 0x26) >> 16) & 0x3fff), 17);
    }

    #[test]
    fn evdev_zero_arg_is_rejected_before_device_lookup() {
        assert_eq!(EVIOCGVERSION & 0xff, 1);
        assert_eq!(((EVIOCGVERSION >> 16) & 0x3fff), 4);
    }
}

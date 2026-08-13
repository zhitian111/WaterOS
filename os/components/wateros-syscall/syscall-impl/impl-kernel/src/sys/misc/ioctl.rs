//! `ioctl(2)`：优先按 fd 句柄分发；RTC 与 TTY 兼容 fallback。

//! 本模块代码由AI完成
use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use vfs::api::{VfsError, VfsFramebufferInfo, VfsInputDeviceInfo, VfsSpecialDeviceInfo};

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
const FBIOGET_VSCREENINFO : u32 = 0x4600;
const FBIOPUT_VSCREENINFO : u32 = 0x4601;
const FBIOGET_FSCREENINFO : u32 = 0x4602;
const FBIOGETCMAP : u32 = 0x4604;
const FBIOPUTCMAP : u32 = 0x4605;
const FBIOPAN_DISPLAY : u32 = 0x4606;

const EVDEV_IOCTL_TYPE : u32 = b'E' as u32;
const EVIOCGVERSION_NR : u32 = 0x01;
const EVIOCGID_NR : u32 = 0x02;
const EVIOCGNAME_NR : u32 = 0x06;
const EVIOCGBIT_BASE_NR : u32 = 0x20;
const EVIOCGABS_BASE_NR : u32 = 0x40;

fn ioctl_req(raw: usize) -> u32 {
    raw as u32
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct LinuxFbBitfield { offset : u32, length : u32, msb_right : u32 }

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxFbFixScreenInfo {
    id : [u8; 16],
    smem_start : usize,
    smem_len : u32,
    fb_type : u32,
    type_aux : u32,
    visual : u32,
    xpanstep : u16,
    ypanstep : u16,
    ywrapstep : u16,
    line_length : u32,
    mmio_start : usize,
    mmio_len : u32,
    accel : u32,
    capabilities : u16,
    reserved : [u16; 2],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxFbVarScreenInfo {
    xres : u32,
    yres : u32,
    xres_virtual : u32,
    yres_virtual : u32,
    xoffset : u32,
    yoffset : u32,
    bits_per_pixel : u32,
    grayscale : u32,
    red : LinuxFbBitfield,
    green : LinuxFbBitfield,
    blue : LinuxFbBitfield,
    transp : LinuxFbBitfield,
    nonstd : u32,
    activate : u32,
    height : u32,
    width : u32,
    accel_flags : u32,
    pixclock : u32,
    left_margin : u32,
    right_margin : u32,
    upper_margin : u32,
    lower_margin : u32,
    hsync_len : u32,
    vsync_len : u32,
    sync : u32,
    vmode : u32,
    rotate : u32,
    colorspace : u32,
    reserved : [u32; 4],
}

const _ : () = assert!(core::mem::size_of::<LinuxFbFixScreenInfo>() == 80);
const _ : () = assert!(core::mem::size_of::<LinuxFbVarScreenInfo>() == 160);

fn fb_var(info : VfsFramebufferInfo) -> LinuxFbVarScreenInfo {
    LinuxFbVarScreenInfo { xres : info.width,
                           yres : info.height,
                           xres_virtual : info.width,
                           yres_virtual : info.height,
                           xoffset : 0,
                           yoffset : 0,
                           bits_per_pixel : 32,
                           grayscale : 0,
                           red : LinuxFbBitfield { offset : 16, length : 8, msb_right : 0 },
                           green : LinuxFbBitfield { offset : 8, length : 8, msb_right : 0 },
                           blue : LinuxFbBitfield { offset : 0, length : 8, msb_right : 0 },
                           transp : LinuxFbBitfield { offset : 24, length : 8, msb_right : 0 },
                           nonstd : 0,
                           activate : 0,
                           height : u32::MAX,
                           width : u32::MAX,
                           accel_flags : 0,
                           pixclock : 0,
                           left_margin : 0,
                           right_margin : 0,
                           upper_margin : 0,
                           lower_margin : 0,
                           hsync_len : 0,
                           vsync_len : 0,
                           sync : 0,
                           vmode : 0,
                           rotate : 0,
                           colorspace : 0,
                           reserved : [0; 4] }
}

fn framebuffer_ioctl(fd : usize,
                      request : u32,
                      argp : usize,
                      info : VfsFramebufferInfo)
                      -> UserRet {
    if argp == 0 { return UserRet::from_error(ErrNo::EFAULT); }
    match request {
        FBIOGET_FSCREENINFO => {
            let mut id = [0u8; 16];
            id[..14].copy_from_slice(b"WaterOS VirtIO");
            let fix = LinuxFbFixScreenInfo { id,
                                             smem_start : info.phys_base,
                                             smem_len : info.byte_len.min(u32::MAX as usize) as u32,
                                             fb_type : 0,
                                             type_aux : 0,
                                             visual : 2,
                                             xpanstep : 0,
                                             ypanstep : 0,
                                             ywrapstep : 0,
                                             line_length : info.stride.min(u32::MAX as usize) as u32,
                                             mmio_start : 0,
                                             mmio_len : 0,
                                             accel : 0,
                                             capabilities : 0,
                                             reserved : [0; 2] };
            copy_to_user_struct(argp, &fix).map_or_else(UserRet::from_error,
                                                        |_| UserRet::from_success(0))
        }
        FBIOGET_VSCREENINFO => copy_to_user_struct(argp, &fb_var(info))
            .map_or_else(UserRet::from_error, |_| UserRet::from_success(0)),
        FBIOPUT_VSCREENINFO | FBIOPAN_DISPLAY => {
            let requested = match copy_from_user_struct::<LinuxFbVarScreenInfo>(argp) {
                Ok(value) => value,
                Err(error) => return UserRet::from_error(error),
            };
            if requested.xres != info.width || requested.yres != info.height ||
               requested.xres_virtual != info.width || requested.yres_virtual != info.height ||
               requested.xoffset != 0 || requested.yoffset != 0 || requested.bits_per_pixel != 32
            {
                return UserRet::from_error(ErrNo::EINVAL);
            }
            if request == FBIOPUT_VSCREENINFO {
                UserRet::from_success(0)
            } else {
                match vfs::fd::with_current_io(fd, |handle| handle.flush_device()) {
                    Ok(()) => UserRet::from_success(0),
                    Err(error) => UserRet::from_error(vfs_error_to_errno(error)),
                }
            }
        }
        FBIOGETCMAP | FBIOPUTCMAP => UserRet::from_error(ErrNo::ENOTTY),
        _ => UserRet::from_error(ErrNo::ENOTTY),
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxInputId { bustype : u16, vendor : u16, product : u16, version : u16 }

#[repr(C)]
#[derive(Clone, Copy)]
struct LinuxInputAbsInfo {
    value : i32,
    minimum : i32,
    maximum : i32,
    fuzz : i32,
    flat : i32,
    resolution : i32,
}

fn set_bit(bytes : &mut [u8], bit : usize) {
    if bit / 8 < bytes.len() { bytes[bit / 8] |= 1u8 << (bit % 8); }
}

fn copy_evdev_bits(argp : usize, requested_len : usize, bits : impl IntoIterator<Item = usize>) -> UserRet {
    let len = requested_len.min(128);
    let mut bytes = alloc::vec![0u8; len];
    for bit in bits { set_bit(&mut bytes, bit); }
    match copy_to_user(argp, &bytes) {
        Ok(copied) => UserRet::from_success(copied),
        Err(error) => UserRet::from_error(error),
    }
}

fn evdev_ioctl(request : u32, argp : usize, info : VfsInputDeviceInfo) -> UserRet {
    if argp == 0 { return UserRet::from_error(ErrNo::EFAULT); }
    let ioctl_type = (request >> 8) & 0xff;
    let nr = request & 0xff;
    let size = ((request >> 16) & 0x3fff) as usize;
    if ioctl_type != EVDEV_IOCTL_TYPE { return UserRet::from_error(ErrNo::ENOTTY); }
    match nr {
        EVIOCGVERSION_NR => copy_to_user_struct(argp, &0x0001_0001i32)
            .map_or_else(UserRet::from_error, |_| UserRet::from_success(0)),
        EVIOCGID_NR => {
            let id = LinuxInputId { bustype : 0x06, vendor : 0, product : 0, version : 1 };
            copy_to_user_struct(argp, &id)
                .map_or_else(UserRet::from_error, |_| UserRet::from_success(0))
        }
        EVIOCGNAME_NR => {
            let mut name = info.name.as_bytes().to_vec();
            name.push(0);
            name.truncate(size);
            match copy_to_user(argp, &name) {
                Ok(copied) => UserRet::from_success(copied),
                Err(error) => UserRet::from_error(error),
            }
        }
        nr if nr >= EVIOCGBIT_BASE_NR && nr < EVIOCGABS_BASE_NR => {
            let event_type = nr - EVIOCGBIT_BASE_NR;
            match event_type {
                0 => {
                    let mut events = alloc::vec![0usize, 1];
                    if info.pointer { events.push(3); }
                    if info.keyboard { events.push(4); events.push(20); }
                    copy_evdev_bits(argp, size, events)
                }
                1 if info.keyboard => copy_evdev_bits(argp, size, 1usize..=255),
                1 if info.pointer => copy_evdev_bits(argp, size, [0x110usize, 0x14a]),
                3 if info.pointer => copy_evdev_bits(argp, size, [0usize, 1usize]),
                _ => copy_evdev_bits(argp, size, core::iter::empty()),
            }
        }
        nr if nr == EVIOCGABS_BASE_NR || nr == EVIOCGABS_BASE_NR + 1 => {
            let range = if nr == EVIOCGABS_BASE_NR { info.absolute_x } else { info.absolute_y };
            let Some((minimum, maximum)) = range else {
                return UserRet::from_error(ErrNo::EINVAL);
            };
            let abs = LinuxInputAbsInfo { value : minimum,
                                          minimum,
                                          maximum,
                                          fuzz : 0,
                                          flat : 0,
                                          resolution : 0 };
            copy_to_user_struct(argp, &abs)
                .map_or_else(UserRet::from_error, |_| UserRet::from_success(0))
        }
        _ => UserRet::from_error(ErrNo::ENOTTY),
    }
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

    // FIONBIO 是所有打开文件描述都可使用的通用状态操作，必须先于
    // framebuffer/evdev 的设备专用 ioctl 分派。
    if request == FIONBIO {
        return fd_fionbio(fd, argp);
    }

    let special = vfs::fd::with_current_io(fd, |handle| Ok(handle.special_device_info()))
        .ok()
        .flatten();
    match special {
        Some(VfsSpecialDeviceInfo::Framebuffer(info)) => {
            return framebuffer_ioctl(fd, request, argp, info);
        }
        Some(VfsSpecialDeviceInfo::InputEvent(info)) => {
            return evdev_ioctl(request, argp, info);
        }
        None => {}
    }

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

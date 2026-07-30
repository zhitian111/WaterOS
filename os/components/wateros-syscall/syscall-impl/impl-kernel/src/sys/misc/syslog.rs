//! `syslog(2)` → 内核 klog 环。

//! 本模块代码由AI完成
use api_v0::ErrNo;
use api_v0::SyscallArgs;
use api_v0::UserRet;
use klog::api::is_write_priority;

use crate::user_copy::{copy_from_user, copy_to_user};

const KBUF_SIZE: usize = 2048;
const SYSLOG_ACTION_READ: i32 = 2;
const SYSLOG_ACTION_READ_ALL: i32 = 3;
const SYSLOG_ACTION_READ_CLEAR: i32 = 4;

/// `syslog(type, buf, len)`（RISC-V：`a0`/`a1`/`a2`）。
// 本方法代码由AI完成
pub(crate) fn sys_syslog(args: SyscallArgs) -> UserRet {
    let action = args.arg(0) as i32;
    let buf_ptr = args.arg(1);
    let len = args.arg(2);

    let mut kbuf = [0u8; KBUF_SIZE];
    let klen = len.min(KBUF_SIZE);

    if is_write_priority(action) {
        if len > 0 && buf_ptr == 0 {
            return UserRet::from_error(ErrNo::EFAULT);
        }
        if len > 0 {
            match copy_from_user(&mut kbuf[..klen], buf_ptr) {
                Ok(n) => {
                    let ret = klog::syscall::dispatch_kernel(action, &mut kbuf, n);
                    return UserRet::from_success(ret.max(0) as usize);
                }
                Err(e) => return UserRet::from_error(e),
            }
        }
        let ret = klog::syscall::dispatch_kernel(action, &mut kbuf, 0);
        return UserRet::from_success(ret.max(0) as usize);
    }

    let ret = klog::syscall::dispatch_kernel(action, &mut kbuf, klen);

    let needs_copy_out = action == SYSLOG_ACTION_READ
        || action == SYSLOG_ACTION_READ_CLEAR
        || action == SYSLOG_ACTION_READ_ALL;
    if needs_copy_out && ret > 0 {
        if buf_ptr == 0 {
            return UserRet::from_error(ErrNo::EFAULT);
        }
        match copy_to_user(buf_ptr, &kbuf[..ret as usize]) {
            Ok(_n) => UserRet::from_success(ret.max(0) as usize),
            Err(e) => UserRet::from_error(e),
        }
    } else {
        UserRet::from_success(ret.max(0) as usize)
    }
}
